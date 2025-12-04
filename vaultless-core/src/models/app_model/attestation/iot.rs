use super::types::{AttestationResult, Platform};
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{SIGNATURE_LENGTH, Signature, VerifyingKey};
use getrandom;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::convert::TryInto;
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;
use x509_parser::prelude::*;

const IOT_CHALLENGE_KEY: &str = "iot_challenge";

// =============================================================================
// IoT ATTESTATION REQUEST
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IoTAttestationRequest {
    #[validate(length(min = 100, max = 8192))]
    pub device_certificate: String,

    #[validate(length(min = 32, max = 2048))]
    pub challenge_signature: String,

    #[validate(length(min = 32, max = 128))]
    pub challenge: String,

    #[validate(length(min = 1, max = 32))]
    pub device_id: String,
}

// =============================================================================
// IoT CERTIFICATE VERIFICATION WITH REVOCATION
// =============================================================================
pub async fn verify_iot_certificate(
    device_certificate: &str,
    challenge_signature: &str,
    challenge: &str,
    device_id: &str,
    allowed_certificate_authorities: &[String],
    require_cn_match: bool,
    postgres_pool: Arc<PgPool>,
    application_id: Uuid,
) -> Result<AttestationResult> {
    let mut warnings: Vec<String> = Vec::new();

    // ---------------------------
    // 1. Calculate certificate hash
    // ---------------------------

    let cert_der = BASE64.decode(device_certificate).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid certificate base64: {}", e))
    })?;

    let cert_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&cert_der);
        hex::encode(hasher.finalize())
    };

    // ---------------------------
    // 2. CHECK DEVICE REVOCATION (POSTGRES)
    // ---------------------------
    let revoked = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT 1
        FROM iot_device_revocations
        WHERE application_id = $1
          AND device_certificate_hash = $2
        LIMIT 1
        "#,
    )
    .bind(application_id)
    .bind(&cert_hash)
    .fetch_optional(&*postgres_pool)
    .await
    .map_err(|e| VaultlessError::Internal(format!("Database query failed: {}", e)))?;

    if revoked.is_some() {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: cert_hash,
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("DEVICE_REVOKED".into()),
            error: Some("This IoT device certificate has been revoked".into()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // ---------------------------
    // 3. Decode device certificate
    // ---------------------------
    let cert_der = BASE64.decode(device_certificate).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid certificate base64: {}", e))
    })?;

    let (_, cert) = X509Certificate::from_der(&cert_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse certificate: {}", e))
    })?;

    // ---------------------------
    // 4. SECURITY CHECK: KeyUsage digitalSignature
    // ---------------------------
    if let Ok(Some(ku)) = cert.key_usage() {
        if !ku.value.digital_signature() {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: cert_hash,
                bundle_id: device_id.to_string(),
                platform: Platform::IoT,
                device_trusted: false,
                verdict: Some("CERTIFICATE_KEYUSAGE_INVALID".into()),
                error: Some("Certificate KeyUsage missing digitalSignature".into()),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        }
    } else {
        warnings.push("Certificate missing KeyUsage extension".to_string());
    }

    // ---------------------------
    // 5. Validity period check
    // ---------------------------
    if !cert.validity().is_valid() {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: cert_hash,
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("CERTIFICATE_EXPIRED".into()),
            error: Some("Certificate not within valid date range".into()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // ---------------------------
    // 6. Verify CA signature
    // ---------------------------
    let tbs = cert.tbs_certificate.as_ref();
    let cert_sig_bytes = cert.signature_value.data.as_ref();
    type SigArray = [u8; SIGNATURE_LENGTH];
    let cert_sig: Signature = match TryInto::<SigArray>::try_into(cert_sig_bytes) {
        Ok(bytes_64) => Signature::from(bytes_64),
        Err(_) => {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: cert_hash,
                bundle_id: device_id.to_string(),
                platform: Platform::IoT,
                device_trusted: false,
                verdict: Some("INVALID_CERTIFICATE_SIGNATURE_LENGTH".into()),
                error: Some(format!(
                    "Certificate signature is not {} bytes",
                    SIGNATURE_LENGTH
                )),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        }
    };

    let mut signed_by_allowed = false;
    for ca_b64 in allowed_certificate_authorities {
        let ca_der = match BASE64.decode(ca_b64) {
            Ok(x) => x,
            Err(_) => continue,
        };
        if let Ok((_, ca_cert)) = X509Certificate::from_der(&ca_der) {
            if let Ok(Some(bc)) = ca_cert.basic_constraints() {
                if !bc.value.ca {
                    warnings.push(format!(
                        "Allowed CA '{}' lacks Basic Constraints cA=true",
                        ca_cert.subject()
                    ));
                    continue;
                }
            } else {
                warnings.push(format!(
                    "Allowed CA '{}' missing Basic Constraints extension",
                    ca_cert.subject()
                ));
                continue;
            }
            let ca_spki_der = ca_cert.tbs_certificate.subject_pki.raw;
            if let Ok(ca_key) = VerifyingKey::from_public_key_der(ca_spki_der)
                && ca_key.verify_strict(tbs, &cert_sig).is_ok()
            {
                signed_by_allowed = true;
                break;
            }
        }
    }

    if !signed_by_allowed {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: cert_hash,
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("CA_NOT_AUTHORIZED".into()),
            error: Some("Certificate not signed by allowed Root CA".to_string()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // ---------------------------
    // 7. Validate CN against device_id
    // ---------------------------
    let device_cn = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .unwrap_or(device_id)
        .to_string();

    if require_cn_match && device_cn != device_id {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: cert_hash,
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("DEVICE_ID_MISMATCH".into()),
            error: Some(format!(
                "Certificate CN ('{}') does not match device_id ('{}')",
                device_cn, device_id
            )),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // ---------------------------
    // 8. Verify challenge signature (Proof of Possession)
    // ---------------------------
    let device_spki_der = cert.tbs_certificate.subject_pki.raw;
    let device_key = VerifyingKey::from_public_key_der(device_spki_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!(
            "Device public key invalid Ed25519 DER: {}",
            e
        ))
    })?;

    let sig_bytes = BASE64.decode(challenge_signature).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid signature base64: {}", e))
    })?;

    let sig: Signature = match TryInto::<SigArray>::try_into(sig_bytes.as_slice()) {
        Ok(bytes_64) => Signature::from(bytes_64),
        Err(_) => {
            return Err(VaultlessError::IntegrityCheckFailed(format!(
                "Challenge signature is not {} bytes",
                SIGNATURE_LENGTH
            )));
        }
    };

    if device_key
        .verify_strict(challenge.as_bytes(), &sig)
        .is_err()
    {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: cert_hash,
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("CHALLENGE_VERIFICATION_FAILED".into()),
            error: Some("Device failed proof-of-possession".into()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // ---------------------------
    // 9. SUCCESS
    // ---------------------------
    Ok(AttestationResult {
        is_valid: true,
        certificate_hash: cert_hash,
        bundle_id: device_cn,
        platform: Platform::IoT,
        device_trusted: true,
        verdict: Some("CERTIFICATE_VALID; CHALLENGE_OK".into()),
        error: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        verified_at: Utc::now(),
    })
}

// =============================================================================
// CHALLENGE GENERATION
// =============================================================================

pub async fn generate_iot_challenge(
    redis_pool: &RedisPool,
    challenge_ttl_seconds: u64,
) -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| VaultlessError::Internal(format!("Random generation failed: {}", e)))?;

    let challenge = BASE64.encode(bytes);

    let challenge_hash = {
        let mut hasher = Sha256::new();
        hasher.update(challenge.as_bytes());
        hex::encode(hasher.finalize())
    };

    let key = cache_key!(IOT_CHALLENGE_KEY, challenge_hash);

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let _: () = conn
        .set_ex::<_, _, ()>(&key, "1", challenge_ttl_seconds)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis SETEX failed: {}", e)))?;

    Ok(challenge)
}
