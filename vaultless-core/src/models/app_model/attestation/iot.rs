use super::types::{AttestationResult, IoTData, PlatformAttestationData};
use crate::error::{Result, VaultlessError};
use crate::models::app_model::attestation::dto::IoTIntegrityConfig;

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
// IoT CERTIFICATE VERIFICATION WITH COMPREHENSIVE VALIDATION
// =============================================================================

pub async fn verify_iot_certificate(
    device_id: Option<&str>,
    device_certificate: &str,
    challenge_signature: &str,
    challenge: &str,
    application_id: Uuid,
    postgres_pool: Arc<PgPool>,
    config: &IoTIntegrityConfig,
) -> Result<AttestationResult> {
    let mut warnings: Vec<String> = Vec::new();

    // ---------------------------
    // 1. Decode certificate
    // ---------------------------
    let cert_der = BASE64.decode(device_certificate).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid certificate base64: {}", e))
    })?;

    let cert_hash = {
        let mut h = Sha256::new();
        h.update(&cert_der);
        hex::encode(h.finalize())
    };

    // ---------------------------
    // 2. Parse certificate
    // ---------------------------
    let (_, cert) = X509Certificate::from_der(&cert_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse certificate: {}", e))
    })?;

    // ---------------------------
    // 3. Extract device CN
    // ---------------------------
    let device_cn = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Certificate missing CN".into()))?
        .to_string();

    // ---------------------------
    // 4. Check iot_devices table
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
    .fetch_optional(&*postgres_pool)
    .await?;

    if device_record.is_none() {
        return Ok(attestation_failure(
            "DEVICE_NOT_REGISTERED",
            "Device not registered in system",
            &device_cn,
            challenge_signature,
            warnings,
        ));
    }

    let device = device_record.as_ref().unwrap();

    // Check device status
    if device.status != "active" {
        return Ok(attestation_failure(
            "DEVICE_REVOKED_OR_INACTIVE",
            &format!("Device status is '{}', expected 'active'", device.status),
            &device_cn,
            challenge_signature,
            warnings,
        ));
    }

    // ---------------------------
    // 5. Check revocation table
    // ---------------------------
    let revoked = sqlx::query_scalar::<_, i64>(
        r#"SELECT 1 FROM iot_device_revocations 
           WHERE application_id = $1 AND device_certificate_hash = $2 LIMIT 1"#,
    )
    .bind(application_id)
    .bind(&cert_hash)
    .fetch_optional(&*postgres_pool)
    .await?;

    if revoked.is_some() {
        return Ok(attestation_failure(
            "DEVICE_REVOKED_OR_INACTIVE",
            "Device certificate is revoked",
            &device_cn,
            challenge_signature,
            warnings,
        ));
    }

    // ---------------------------
    // 6. CN Match Validation
    // ---------------------------
    if config.require_cn_match {
        if let Some(id_value) = device_id {
            if device_cn.as_str() != id_value {
                return Ok(attestation_failure(
                    "CN_MISMATCH",
                    &format!(
                        "Device CN mismatch: expected '{}', got '{}'",
                        id_value, device_cn
                    ),
                    &device_cn,
                    challenge_signature,
                    warnings,
                ));
            }
        }

        warnings.push(format!("CN match verified: {}", device_cn));
    }

    // ---------------------------
    // 7. Secure Element ID Validation
    // ---------------------------
    if !config.allowed_secure_element_ids.is_empty() {
        let device_se_id = device.secure_element_id.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "Device missing secure element ID but config requires it".into(),
            )
        })?;

        if !config.allowed_secure_element_ids.contains(device_se_id) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_SECURE_ELEMENT",
                &format!("Secure element '{}' not in allowlist", device_se_id),
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }

        warnings.push(format!("Secure element verified: {}", device_se_id));
    }

    // ---------------------------
    // 8. Manufacturer Validation
    // ---------------------------
    if !config.allowed_manufacturers.is_empty() {
        let device_manufacturer = device.manufacturer.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "Device missing manufacturer but config requires it".into(),
            )
        })?;

        if !config.allowed_manufacturers.contains(device_manufacturer) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_MANUFACTURER",
                &format!("Manufacturer '{}' not in allowlist", device_manufacturer),
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 9. Model Validation
    // ---------------------------
    if !config.allowed_models.is_empty() {
        let device_model = device.model.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "Device missing model but config requires it".into(),
            )
        })?;

        if !config.allowed_models.contains(device_model) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_MODEL",
                &format!("Model '{}' not in allowlist", device_model),
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 10. Hardware Revision Validation
    // ---------------------------
    if !config.allowed_hardware_revisions.is_empty() {
        let device_hw_rev = device.hardware_revision.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "Device missing hardware revision but config requires it".into(),
            )
        })?;

        if !config.allowed_hardware_revisions.contains(device_hw_rev) {
            return Ok(attestation_failure(
                "UNAUTHORIZED_HARDWARE_REVISION",
                &format!("Hardware revision '{}' not in allowlist", device_hw_rev),
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 11. Firmware Version Validation
    // ---------------------------
    if let Some(min_version) = config.min_firmware_version {
        let device_fw_version = device.firmware_version.as_ref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "Device missing firmware version but config requires it".into(),
            )
        })?;

        // Parse firmware version (assuming format like "305" or "3.0.5")
        let fw_version_num: i32 = device_fw_version.parse().map_err(|_| {
            VaultlessError::IntegrityCheckFailed(format!(
                "Invalid firmware version format: {}",
                device_fw_version
            ))
        })?;

        if fw_version_num < min_version {
            return Ok(attestation_failure(
                "FIRMWARE_VERSION_TOO_OLD",
                &format!(
                    "Firmware version {} is below minimum required {}",
                    fw_version_num, min_version
                ),
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 12. Device Idle Time Check
    // ---------------------------
    if let Some(max_idle_secs) = config.max_device_idle_seconds {
        if let Some(last_seen) = device.last_seen {
            let idle_duration = Utc::now().signed_duration_since(last_seen);
            let idle_seconds = idle_duration.num_seconds();

            if idle_seconds > max_idle_secs as i64 {
                let warning = format!(
                    "Device idle for {} seconds (max allowed: {})",
                    idle_seconds, max_idle_secs
                );

                if config.strict_mode {
                    return Ok(attestation_failure(
                        "DEVICE_IDLE_TOO_LONG",
                        &warning,
                        &device_cn,
                        challenge_signature,
                        warnings,
                    ));
                } else {
                    warnings.push(warning);
                }
            }
        }
    }

    // ---------------------------
    // 13. Certificate KeyUsage Validation
    // ---------------------------
    if let Ok(Some(ku)) = cert.key_usage() {
        if !ku.value.digital_signature() {
            let msg = "Certificate KeyUsage missing digitalSignature";
            if config.strict_mode {
                return Ok(attestation_failure(
                    "INVALID_KEY_USAGE",
                    msg,
                    &device_cn,
                    challenge_signature,
                    warnings,
                ));
            } else {
                warnings.push(msg.into());
            }
        }
    } else {
        let msg = "Certificate missing KeyUsage extension";
        if config.strict_mode {
            return Ok(attestation_failure(
                "MISSING_KEY_USAGE",
                msg,
                &device_cn,
                challenge_signature,
                warnings,
            ));
        } else {
            warnings.push(msg.into());
        }
    }

    // ---------------------------
    // 14. Certificate Validity Period
    // ---------------------------
    if config.require_valid_certificate_expiry && !cert.validity().is_valid() {
        return Ok(attestation_failure(
            "CERTIFICATE_EXPIRED",
            "Certificate expired or not yet valid",
            &device_cn,
            challenge_signature,
            warnings,
        ));
    }

    if config.reject_future_certificates {
        let now = ASN1Time::now();
        if cert.validity().not_before > now {
            return Ok(attestation_failure(
                "CERTIFICATE_NOT_YET_VALID",
                "Certificate not_before date is in the future",
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 15. SAN (SubjectAlternativeName) Validation
    // ---------------------------
    if !config.required_san_fields.is_empty() {
        if let Ok(Some(san_ext)) = cert.subject_alternative_name() {
            let san_entries: Vec<String> = san_ext
                .value
                .general_names
                .iter()
                .filter_map(|gn| match gn {
                    GeneralName::DNSName(name) => Some(format!("DNS:{}", name)),
                    GeneralName::URI(uri) => Some(format!("URI:{}", uri)),
                    GeneralName::RFC822Name(email) => Some(format!("EMAIL:{}", email)),
                    GeneralName::IPAddress(ip) => Some(format!("IP:{}", hex::encode(ip))),
                    _ => None,
                })
                .collect();

            for required_san in &config.required_san_fields {
                if !san_entries.iter().any(|entry| entry.contains(required_san)) {
                    return Ok(attestation_failure(
                        "MISSING_REQUIRED_SAN",
                        &format!("Certificate missing required SAN field: {}", required_san),
                        &device_cn,
                        challenge_signature,
                        warnings,
                    ));
                }
            }
        } else {
            return Ok(attestation_failure(
                "MISSING_SAN_EXTENSION",
                "Certificate missing SubjectAlternativeName extension",
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 16. Certificate Authority Validation
    // ---------------------------
    if !config.allowed_certificate_authorities.is_empty() {
        let tbs = cert.tbs_certificate.as_ref();
        let cert_sig_bytes = cert.signature_value.data.as_ref();

        let cert_sig: Signature = TryInto::<[u8; SIGNATURE_LENGTH]>::try_into(cert_sig_bytes)
            .map_err(|_| {
                VaultlessError::IntegrityCheckFailed("Invalid certificate signature length".into())
            })?
            .into();

        let mut signed_by_allowed = false;

        for ca_b64 in &config.allowed_certificate_authorities {
            if let Ok(ca_der) = BASE64.decode(ca_b64) {
                if let Ok((_, ca_cert)) = X509Certificate::from_der(&ca_der) {
                    // Verify this is actually a CA certificate
                    if let Ok(Some(bc)) = ca_cert.basic_constraints() {
                        if !bc.value.ca {
                            continue;
                        }
                    }

                    // Try to verify signature with this CA
                    if let Ok(ca_key) =
                        VerifyingKey::from_public_key_der(ca_cert.tbs_certificate.subject_pki.raw)
                    {
                        if ca_key.verify_strict(tbs, &cert_sig).is_ok() {
                            signed_by_allowed = true;
                            break;
                        }
                    }
                }
            }
        }

        if !signed_by_allowed {
            return Ok(attestation_failure(
                "CA_NOT_AUTHORIZED",
                "Certificate not signed by allowed root CA",
                &device_cn,
                challenge_signature,
                warnings,
            ));
        }
    }

    // ---------------------------
    // 17. Challenge Signature Verification (Proof-of-Possession)
    // ---------------------------
    let public_key_der = cert.tbs_certificate.subject_pki.raw;
    let device_key = VerifyingKey::from_public_key_der(public_key_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Device public key invalid: {}", e))
    })?;

    let sig_bytes = BASE64.decode(challenge_signature).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid signature base64: {}", e))
    })?;

    let sig: Signature = TryInto::<[u8; SIGNATURE_LENGTH]>::try_into(sig_bytes.as_slice())
        .map_err(|_| VaultlessError::IntegrityCheckFailed("Signature invalid length".into()))?
        .into();

    if device_key
        .verify_strict(challenge.as_bytes(), &sig)
        .is_err()
    {
        return Ok(attestation_failure(
            "CHALLENGE_VERIFICATION_FAILED",
            "Device failed proof-of-possession (invalid signature)",
            &device_cn,
            challenge_signature,
            warnings,
        ));
    }

    // ---------------------------
    // 18. Update last_seen timestamp
    // ---------------------------
    sqlx::query!(
        r#"UPDATE iot_devices SET last_seen = NOW() WHERE id = $1"#,
        device.id
    )
    .execute(&*postgres_pool)
    .await?;

    // ---------------------------
    // SUCCESS
    // ---------------------------
    Ok(AttestationResult {
        is_valid: true,
        device_trusted: true,
        verdict: Some("ATTESTATION_SUCCESS".into()),
        error: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        verified_at: Utc::now(),
        platform_data: PlatformAttestationData::IoT(IoTData {
            device_cn,
            firmware_version: device.firmware_version.clone().unwrap_or_default(),
            device_signature: challenge_signature.into(),
        }),
    })
}

// =============================================================================
// FAILURE HELPER
// =============================================================================

fn attestation_failure(
    verdict: &str,
    error: &str,
    device_cn: &str,
    challenge_signature: &str,
    warnings: Vec<String>,
) -> AttestationResult {
    AttestationResult {
        is_valid: false,
        device_trusted: false,
        verdict: Some(verdict.into()),
        error: Some(error.into()),
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        verified_at: Utc::now(),
        platform_data: PlatformAttestationData::IoT(IoTData {
            device_cn: device_cn.into(),
            firmware_version: "".into(),
            device_signature: challenge_signature.into(),
        }),
    }
}
