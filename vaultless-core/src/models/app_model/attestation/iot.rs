use super::types::{AttestationResult, Platform};
use crate::error::{Result, VaultlessError};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{SIGNATURE_LENGTH, Signature, VerifyingKey};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::convert::TryInto;
use std::sync::Arc;
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
// IoT CERTIFICATE VERIFICATION
// =============================================================================

/// Verify IoT device certificate with Ed25519 CA signature validation
pub async fn verify_iot_certificate(
    device_certificate: &str,
    challenge_signature: &str,
    challenge: &str,
    device_id: &str,
    allowed_certificate_authorities: &[String],
    require_cn_match: bool,
    redis_pool: Option<Arc<RedisPool>>,
) -> Result<AttestationResult> {
    let mut warnings: Vec<String> = Vec::new();

    // 1. CRITICAL: Check challenge exists (REPLAY PROTECTION)
    // NOTE: We verify but DON'T delete until after successful verification
    if let Some(pool) = &redis_pool {
        let challenge_hash = {
            let mut hasher = Sha256::new();
            hasher.update(challenge.as_bytes());
            hex::encode(hasher.finalize())
        };
        let cache_key = format!("{}:{}", IOT_CHALLENGE_KEY, challenge_hash);

        let mut conn = pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

        let exists: Option<String> = conn
            .get(&cache_key)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis GET failed: {}", e)))?;

        if exists.is_none() {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: String::new(),
                bundle_id: device_id.to_string(),
                platform: Platform::IoT,
                device_trusted: false,
                verdict: Some("CHALLENGE_EXPIRED_OR_REPLAYED".into()),
                error: Some("Challenge expired, invalid, or already used".into()),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        }
        // NOTE: Challenge will be deleted AFTER all verifications pass
    } else {
        warnings.push("Redis pool not configured; challenge replay protection disabled".into());
    }

    // 2. Decode device certificate
    let cert_der = BASE64.decode(device_certificate).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid certificate base64: {}", e))
    })?;

    let (_, cert) = X509Certificate::from_der(&cert_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse certificate: {}", e))
    })?;

    // 3. SECURITY CHECK: KeyUsage must include digitalSignature
    if let Ok(Some(ku)) = cert.key_usage() {
        if !ku.value.digital_signature() {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: String::new(),
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

    // 4. Validity period check
    if !cert.validity().is_valid() {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: String::new(),
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("CERTIFICATE_EXPIRED".into()),
            error: Some("Certificate not within valid date range".into()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    let issuer_dn = cert.issuer().to_string();

    // 5. Verify CA signature (device cert must be signed by allowed CA)
    let mut signed_by_allowed = false;
    let tbs = cert.tbs_certificate.as_ref();
    let cert_sig_bytes = cert.signature_value.data.as_ref();

    type SigArray = [u8; SIGNATURE_LENGTH];
    let cert_sig: Signature = match TryInto::<SigArray>::try_into(cert_sig_bytes) {
        Ok(bytes_64) => Signature::from(bytes_64),
        Err(_) => {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: String::new(),
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

    for ca_b64 in allowed_certificate_authorities {
        let ca_der = match BASE64.decode(ca_b64) {
            Ok(x) => x,
            Err(_) => continue,
        };

        if let Ok((_, ca_cert)) = X509Certificate::from_der(&ca_der) {
            // SECURITY CHECK: CA must have Basic Constraints cA=true
            if let Ok(Some(basic_constraints)) = ca_cert.basic_constraints() {
                if !basic_constraints.value.ca {
                    warnings.push(format!(
                        "Allowed CA '{}' lacks Basic Constraints cA=true",
                        ca_cert.subject().to_string()
                    ));
                    continue;
                }
            } else {
                warnings.push(format!(
                    "Allowed CA '{}' missing Basic Constraints extension",
                    ca_cert.subject().to_string()
                ));
                continue;
            }

            let ca_spki_der = ca_cert.tbs_certificate.subject_pki.raw;

            if let Ok(ca_key) = VerifyingKey::from_public_key_der(ca_spki_der) {
                if ca_key.verify_strict(tbs, &cert_sig).is_ok() {
                    signed_by_allowed = true;
                    break;
                }
            }
        }
    }

    if !signed_by_allowed {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: String::new(),
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("CA_NOT_AUTHORIZED".into()),
            error: Some(format!(
                "Certificate not signed by allowed Root CA (issuer: {})",
                issuer_dn
            )),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // 6. Extract device CN and validate against device_id
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
            certificate_hash: String::new(),
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

    // 7. Verify challenge signature (PROOF OF POSSESSION)
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

    // CRITICAL: Verify signature BEFORE deleting challenge
    if device_key
        .verify_strict(challenge.as_bytes(), &sig)
        .is_err()
    {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: String::new(),
            bundle_id: device_id.to_string(),
            platform: Platform::IoT,
            device_trusted: false,
            verdict: Some("CHALLENGE_VERIFICATION_FAILED".into()),
            error: Some("Device failed proof-of-possession".into()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // 8. Calculate certificate hash
    let cert_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&cert_der);
        hex::encode(hasher.finalize())
    };

    // 9. CRITICAL: Delete challenge ONLY AFTER successful verification
    if let Some(pool) = redis_pool {
        let challenge_hash = {
            let mut hasher = Sha256::new();
            hasher.update(challenge.as_bytes());
            hex::encode(hasher.finalize())
        };
        let cache_key = format!("{}:{}", IOT_CHALLENGE_KEY, challenge_hash);

        let mut conn = pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

        let _: () = conn
            .del(&cache_key)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis DEL failed: {}", e)))?;
    }

    // 10. Success
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
    getrandom::getrandom(&mut bytes)
        .map_err(|e| VaultlessError::Internal(format!("Random generation failed: {}", e)))?;

    let challenge = BASE64.encode(bytes);

    let challenge_hash = {
        let mut hasher = Sha256::new();
        hasher.update(challenge.as_bytes());
        hex::encode(hasher.finalize())
    };

    let key = format!("{}:{}", IOT_CHALLENGE_KEY, challenge_hash);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iot_attestation_request_validation() {
        let req = IoTAttestationRequest {
            device_certificate: BASE64.encode(vec![0u8; 100]),
            challenge_signature: BASE64.encode(vec![0u8; 64]),
            challenge: BASE64.encode(vec![0u8; 32]),
            device_id: "device-123".to_string(),
        };

        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_challenge_hash_consistency() {
        let challenge = "test-challenge-12345";
        let hash1 = {
            let mut hasher = Sha256::new();
            hasher.update(challenge.as_bytes());
            hex::encode(hasher.finalize())
        };

        let hash2 = {
            let mut hasher = Sha256::new();
            hasher.update(challenge.as_bytes());
            hex::encode(hasher.finalize())
        };

        assert_eq!(hash1, hash2);
    }
}
