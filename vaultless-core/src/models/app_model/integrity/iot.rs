use super::types::AttestationResult;
use crate::error::{Result, VaultlessError};
use crate::models::app_model::integrity::dto::IoTIntegrityConfig;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;

use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{SIGNATURE_LENGTH, Signature, VerifyingKey};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::convert::TryInto;
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

use x509_parser::prelude::*;

// =============================================================================
// ATTESTATION REQUEST STRUCT
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IoTAttestationRequest {
    #[validate(length(min = 100, max = 8192))]
    pub device_certificate: String,

    #[validate(length(min = 32, max = 2048))]
    pub challenge_signature: String,

    #[validate(length(min = 32, max = 128))]
    pub challenge: String,
}

// =============================================================================
// IoT CERTIFICATE VERIFICATION WITH CONFIDENCE SCORING
// =============================================================================
pub async fn verify_iot_certificate(
    device_id: Option<&str>,
    device_certificate: &str,
    challenge_signature: Option<&str>,
    challenge: Option<&str>,
    application_id: Uuid,
    config: &IoTIntegrityConfig,
    postgres_pool: Arc<PgPool>,
) -> Result<AttestationResult> {
    let mut warnings: Vec<String> = Vec::new();
    let mut confidence: u8 = 0;

    // ---------------------------
    // 1. Decode certificate
    // ---------------------------
    let cert_der = BASE64.decode(device_certificate).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid certificate base64: {}", e))
    })?;
    confidence += 5;

    let cert_hash = {
        let mut h = Sha256::new();
        h.update(&cert_der);
        hex::encode(h.finalize())
    };

    //  Parse certificate

    let (_, cert) = X509Certificate::from_der(&cert_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse certificate: {}", e))
    })?;
    confidence += 5;

    // Certificate Authority validation

    let issuer_cn = cert
        .issuer()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(|s| s.to_string());

    if !config.allowed_certificate_authorities.is_empty() {
        let issuer = issuer_cn.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Certificate missing issuer CN".into())
        })?;

        if !config.allowed_certificate_authorities.contains(issuer) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_CERTIFICATE_AUTHORITY",
                "Certificate authority not allowed",
                confidence,
                warnings,
            ));
        }
        confidence += 10;
    }

    // Certificate expiry validation

    if config.require_valid_certificate_expiry.unwrap_or(true) {
        let now = Utc::now().timestamp();
        let validity = cert.validity();

        let not_before = validity.not_before.timestamp();
        let not_after = validity.not_after.timestamp();

        if now < not_before {
            return Ok(attestation_failure(
                "CERTIFICATE_NOT_YET_VALID",
                "Certificate not yet valid",
                confidence,
                warnings,
            ));
        }

        if now > not_after {
            return Ok(attestation_failure(
                "CERTIFICATE_EXPIRED",
                "Certificate has expired",
                confidence,
                warnings,
            ));
        }
        confidence += 10;
    }

    // ---------------------------
    // 5. Future certificate rejection
    // ---------------------------
    if config.reject_future_certificates.unwrap_or(true) {
        let now = Utc::now().timestamp();
        let not_before = cert.validity().not_before.timestamp();

        if not_before > now {
            return Ok(attestation_failure(
                "CERTIFICATE_FROM_FUTURE",
                "Certificate notBefore is in the future",
                confidence,
                warnings,
            ));
        }
        confidence += 5;
    }

    // ---------------------------
    // 6. Subject Alternative Name (SAN) validation
    // ---------------------------
    if !config.required_san_fields.is_empty() {
        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME);

        if let Some(ext) = san_ext {
            if let x509_parser::extensions::ParsedExtension::SubjectAlternativeName(san_names) =
                ext.parsed_extension()
            {
                let san_strings: Vec<String> = san_names
                    .general_names
                    .iter()
                    .filter_map(|gn| match gn {
                        x509_parser::extensions::GeneralName::DNSName(s) => Some(s.to_string()),
                        x509_parser::extensions::GeneralName::RFC822Name(s) => Some(s.to_string()),
                        x509_parser::extensions::GeneralName::URI(s) => Some(s.to_string()),
                        _ => None,
                    })
                    .collect();

                for required_san in &config.required_san_fields {
                    if !san_strings.iter().any(|s| s.contains(required_san)) {
                        return Ok(attestation_failure(
                            "MISSING_REQUIRED_SAN",
                            &format!("Required SAN field '{}' not found", required_san),
                            confidence,
                            warnings,
                        ));
                    }
                }
                confidence += 10;
            } else {
                return Ok(attestation_failure(
                    "INVALID_SAN_EXTENSION",
                    "Failed to parse SAN extension content",
                    confidence,
                    warnings,
                ));
            }
        } else {
            return Ok(attestation_failure(
                "MISSING_REQUIRED_SAN",
                "Certificate missing SAN extension",
                confidence,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 7. Extract device CN
    // ---------------------------
    let device_cn = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Certificate missing CN".into()))?
        .to_string();
    confidence += 5;

    // ---------------------------
    // 8. Load device record
    // ---------------------------
    let device_record = sqlx::query!(
        r#"
        SELECT 
            id, 
            status::TEXT as "status!", 
            public_key_der,
            secure_element_id,
            manufacturer,
            model,
            hardware_revision,
            firmware_version,
            last_seen
        FROM iot_devices 
        WHERE application_id = $1 AND device_cn = $2
        "#,
        application_id,
        device_cn
    )
    .fetch_optional(postgres_pool.as_ref())
    .await?;

    if device_record.is_none() {
        return Ok(attestation_failure(
            "DEVICE_NOT_REGISTERED",
            "Device not registered in system",
            confidence,
            warnings,
        ));
    }

    let device = device_record.as_ref().unwrap();

    if device.status != "active" {
        return Ok(attestation_failure(
            "DEVICE_REVOKED_OR_INACTIVE",
            &format!("Device status '{}' not active", device.status),
            confidence,
            warnings,
        ));
    }
    confidence += 10;

    // ---------------------------
    // 9. Revocation check
    // ---------------------------
    let revoked = sqlx::query_scalar::<_, i64>(
        r#"SELECT 1 FROM iot_device_revocations 
           WHERE application_id = $1 AND device_certificate_hash = $2 LIMIT 1"#,
    )
    .bind(application_id)
    .bind(&cert_hash)
    .fetch_optional(postgres_pool.as_ref())
    .await?;

    if revoked.is_some() {
        return Ok(attestation_failure(
            "DEVICE_REVOKED_OR_INACTIVE",
            "Device certificate revoked",
            confidence,
            warnings,
        ));
    }
    confidence += 10;

    // ---------------------------
    // 10. CN Match
    // ---------------------------
    if config.require_cn_match.unwrap_or(true) {
        if let Some(id_value) = device_id {
            if device_cn != id_value {
                return Ok(attestation_failure(
                    "CN_MISMATCH",
                    "Device CN mismatch",
                    confidence,
                    warnings,
                ));
            }
        }
        confidence += 5;
    }

    // ---------------------------
    // 11. Secure Element ID
    // ---------------------------
    if !config.allowed_secure_element_ids.is_empty() {
        let se_id = device.secure_element_id.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Missing secure element ID".into())
        })?;

        if !config.allowed_secure_element_ids.contains(se_id) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_SECURE_ELEMENT",
                "Secure element not allowed",
                confidence,
                warnings,
            ));
        }
        confidence += 10;
    }

    // ---------------------------
    // 12. Manufacturer check
    // ---------------------------
    if !config.allowed_manufacturers.is_empty() {
        let m = device
            .manufacturer
            .as_ref()
            .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing manufacturer".into()))?;

        if !config.allowed_manufacturers.contains(m) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_MANUFACTURER",
                "Manufacturer not allowed",
                confidence,
                warnings,
            ));
        }
        confidence += 5;
    }

    // ---------------------------
    // 13. Model check
    // ---------------------------
    if !config.allowed_models.is_empty() {
        let model = device
            .model
            .as_ref()
            .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing model".into()))?;

        if !config.allowed_models.contains(model) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_MODEL",
                "Model not allowed",
                confidence,
                warnings,
            ));
        }
        confidence += 5;
    }

    // ---------------------------
    // 14. Hardware revision check
    // ---------------------------
    if !config.allowed_hardware_revisions.is_empty() {
        let hw_rev = device.hardware_revision.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Missing hardware revision".into())
        })?;

        if !config.allowed_hardware_revisions.contains(hw_rev) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_HARDWARE_REVISION",
                "Hardware revision not allowed",
                confidence,
                warnings,
            ));
        }
        confidence += 5;
    }

    // ---------------------------
    // 15. Firmware version check
    // ---------------------------
    if let Some(min) = config.min_firmware_version {
        let fw = device.firmware_version.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Missing firmware version".into())
        })?;

        let fw_num: i32 = fw
            .parse()
            .map_err(|_| VaultlessError::IntegrityCheckFailed("Invalid firmware number".into()))?;

        if fw_num < min {
            return Ok(attestation_failure(
                "FIRMWARE_VERSION_TOO_OLD",
                "Firmware too old",
                confidence,
                warnings,
            ));
        }
        confidence += 10;
    }

    // ---------------------------
    // 16. Device idle time check
    // ---------------------------
    if let Some(max_idle) = config.max_device_idle_seconds {
        if let Some(last_seen) = device.last_seen {
            let idle_duration = Utc::now().signed_duration_since(last_seen);
            let idle_seconds = idle_duration.num_seconds() as u64;

            if idle_seconds > max_idle {
                return Ok(attestation_failure(
                    "DEVICE_IDLE_TOO_LONG",
                    &format!(
                        "Device idle for {} seconds (max: {})",
                        idle_seconds, max_idle
                    ),
                    confidence,
                    warnings,
                ));
            }
            confidence += 5;
        } else {
            warnings.push("Device has no last_seen timestamp, skipping idle check".into());
        }
    }

    // ---------------------------
    // 17. Challenge signature verification
    // ---------------------------
    if config.require_challenge_signature.unwrap_or(true) {
        // Ensure both challenge and signature are provided
        let challenge_str = challenge.ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Challenge required but not provided".into())
        })?;

        let challenge_sig = challenge_signature.ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "Challenge signature required but not provided".into(),
            )
        })?;

        let public_key_der = cert.tbs_certificate.subject_pki.raw;
        let device_key = VerifyingKey::from_public_key_der(public_key_der).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid public key: {}", e))
        })?;

        let sig_bytes = BASE64.decode(challenge_sig).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid signature: {}", e))
        })?;

        let sig: Signature = TryInto::<[u8; SIGNATURE_LENGTH]>::try_into(sig_bytes.as_slice())
            .map_err(|_| VaultlessError::IntegrityCheckFailed("Signature length invalid".into()))?
            .into();

        if device_key
            .verify_strict(challenge_str.as_bytes(), &sig)
            .is_err()
        {
            return Ok(attestation_failure(
                "CHALLENGE_VERIFICATION_FAILED",
                "Signature invalid",
                confidence,
                warnings,
            ));
        }
        confidence += 20;
    } else if challenge_signature.is_some() || challenge.is_some() {
        // Optional: warn if signature/challenge provided but not required
        warnings.push("Challenge signature provided but not required by config".into());
    }

    // Cap confidence
    let confidence = confidence.min(100);

    // ---------------------------
    // Update last_seen
    // ---------------------------
    sqlx::query!(
        r#"UPDATE iot_devices SET last_seen = NOW() WHERE id = $1"#,
        device.id
    )
    .execute(postgres_pool.as_ref())
    .await?;

    // Calculate public key fingerprint

    let public_key_fingerprint = {
        let mut h = Sha256::new();
        h.update(cert.tbs_certificate.subject_pki.raw);
        hex::encode(h.finalize())
    };

    // SUCCESS - Build extra metadata

    let extra = serde_json::json!({
        "device_cn": device_cn,
        "issuer_cn": issuer_cn,
        "certificate_hash": cert_hash,
        "public_key_fingerprint": public_key_fingerprint,
        "manufacturer": device.manufacturer,
        "model": device.model,
        "hardware_revision": device.hardware_revision,
        "firmware_version": device.firmware_version,
        "secure_element_id": device.secure_element_id,
        "last_attestation_at": Utc::now().to_rfc3339(),
    });

    Ok(AttestationResult {
        is_valid: true,
        device_trusted: true,
        trust_score_percent: confidence,
        verdict: Some("ATTESTATION_SUCCESS".into()),
        error: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        verified_at: Utc::now(),
        extra,
    })
}

// =============================================================================
// FAILURE HELPER
// =============================================================================

fn attestation_failure(
    verdict: &str,
    error: &str,
    confidence: u8,
    warnings: Vec<String>,
) -> AttestationResult {
    AttestationResult {
        is_valid: false,
        device_trusted: false,
        trust_score_percent: confidence,
        verdict: Some(verdict.into()),
        error: Some(error.into()),
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        verified_at: Utc::now(),
        extra: serde_json::Value::Null,
    }
}
