use crate::cache_key;
use crate::error::{Result, VaultlessError};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use webpki::{EndEntityCert, Time};
use x509_parser::asn1_rs::Tag;
use x509_parser::der_parser::parse_der;
use x509_parser::prelude::*;

use super::types::*;

// =============================================================================
// APPLE ROOT CA CERTIFICATE
// =============================================================================

const APPLE_ROOT_CA_G3: &[u8] = include_bytes!("../../../../certs/AppleRootCA-G3.cer");

const APPLE_APPATTEST_EXTENSION_OID: &str = "1.2.840.113635.100.8.2";
const IOS_CHALLENGE_KEY: &str = "ios_challenge";

// =============================================================================
// APP ATTEST RESPONSE TYPES
// =============================================================================

#[derive(Debug, Deserialize)]
struct AppAttestObject {
    #[serde(rename = "fmt")]
    format: Option<String>,
    #[serde(rename = "attStmt")]
    att_stmt: AttestationStatement,
    #[serde(rename = "authData")]
    auth_data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttestationStatement {
    #[serde(rename = "x5c")]
    x5c: Option<Vec<String>>,
    receipt: Option<String>,
}

// =============================================================================
// CERTIFICATE CHAIN VERIFICATION
// =============================================================================

pub fn verify_apple_certificate_chain(cert_chain: &[String]) -> Result<()> {
    if cert_chain.is_empty() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Certificate chain is empty".into(),
        ));
    }

    let trust_anchor = webpki::TrustAnchor::try_from_cert_der(APPLE_ROOT_CA_G3).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid Apple root CA: {:?}", e))
    })?;

    let leaf_cert_der = BASE64.decode(&cert_chain[0]).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid leaf certificate: {}", e))
    })?;

    let leaf_cert = EndEntityCert::try_from(&leaf_cert_der[..]).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse leaf certificate: {:?}", e))
    })?;

    let mut intermediates_der = Vec::new();
    for cert_b64 in &cert_chain[1..] {
        let cert_der = BASE64.decode(cert_b64).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid intermediate certificate: {}", e))
        })?;
        intermediates_der.push(cert_der);
    }

    let intermediates: Vec<&[u8]> = intermediates_der.iter().map(|c| c.as_slice()).collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| VaultlessError::Internal(format!("System time error: {}", e)))?;

    let time = Time::from_seconds_since_unix_epoch(now.as_secs());

    leaf_cert
        .verify_is_valid_tls_server_cert(
            &[&webpki::ECDSA_P256_SHA256],
            &webpki::TlsServerTrustAnchors(&[trust_anchor]),
            &intermediates,
            time,
        )
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!(
                "Certificate chain verification failed: {:?}",
                e
            ))
        })?;

    Ok(())
}

// =============================================================================
// APP ID VERIFICATION
// =============================================================================

pub fn verify_app_id_from_certificate(
    cert_der: &[u8],
    expected_team_id: &str,
    expected_bundle_id: &str,
) -> Result<()> {
    let (_, cert) = X509Certificate::from_der(cert_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse certificate: {}", e))
    })?;

    for ext in cert.extensions() {
        let oid_str = ext.oid.to_id_string();

        if oid_str == APPLE_APPATTEST_EXTENSION_OID {
            let (_, inner_data) = parse_der(ext.value).map_err(|_| {
                VaultlessError::IntegrityCheckFailed("Failed to parse App ID extension".into())
            })?;

            if inner_data.tag() == Tag::Utf8String {
                let app_id = inner_data
                    .as_str()
                    .map_err(|_| {
                        VaultlessError::IntegrityCheckFailed(
                            "Failed to read App ID as string".into(),
                        )
                    })?
                    .to_string();

                let expected_app_id = format!("{}.{}", expected_team_id, expected_bundle_id);

                if app_id.trim() != expected_app_id {
                    return Err(VaultlessError::IntegrityCheckFailed(format!(
                        "App ID mismatch. Expected: {}, Found: {}",
                        expected_app_id, app_id
                    )));
                }

                return Ok(());
            } else {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "App ID extension value not UTF8 string".into(),
                ));
            }
        }
    }

    Err(VaultlessError::IntegrityCheckFailed(
        "App ID extension not found in certificate".into(),
    ))
}

// =============================================================================
// iOS APP ATTEST VERIFICATION
// =============================================================================
pub async fn verify_ios_attestation(
    token: &str,
    expected_bundle_id: &str,
    expected_team_id: &str,
    allowed_certificate_hashes: &[String],
    reject_untrusted_device: bool,
) -> Result<AttestationResult> {
    let mut warnings = Vec::new();

    // 1. Decode base64 token
    let attestation_bytes = BASE64.decode(token).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid base64 token: {}", e))
    })?;

    // 2. Parse attestation object (CBOR format)
    #[cfg(feature = "cbor")]
    let attestation_obj: AppAttestObject = {
        ciborium::de::from_reader(&attestation_bytes[..]).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!(
                "Invalid attestation format (CBOR): {}",
                e
            ))
        })?
    };

    #[cfg(not(feature = "cbor"))]
    compile_error!("CBOR feature must be enabled for iOS AppAttest verification");

    // 3. Verify format
    if let Some(fmt) = &attestation_obj.format
        && fmt != "apple-appattest"
    {
        warnings.push(format!("Unexpected format: {}", fmt));
    }

    // 4. Extract certificate chain
    let cert_chain = attestation_obj
        .att_stmt
        .x5c
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing certificate chain".into()))?;

    if cert_chain.is_empty() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Empty certificate chain".into(),
        ));
    }

    // 5. Get leaf certificate
    let leaf_cert_b64 = &cert_chain[0];
    let leaf_cert_der = BASE64
        .decode(leaf_cert_b64)
        .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid certificate: {}", e)))?;

    // 6. Calculate SHA-256 of certificate
    let cert_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&leaf_cert_der);
        format!("{:x}", hasher.finalize())
    };

    // 7. Verify certificate hash (if pinning is configured)
    if !allowed_certificate_hashes.is_empty() {
        let cert_match = allowed_certificate_hashes
            .iter()
            .any(|hash| hash.to_lowercase() == cert_hash.to_lowercase());

        if !cert_match {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: cert_hash,
                bundle_id: expected_bundle_id.to_string(),
                platform: Platform::IOS,
                device_trusted: false,
                verdict: Some("CERTIFICATE_HASH_MISMATCH".to_string()),
                error: Some("Certificate not in allowed list".to_string()),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        }
    }

    // 8. Verify certificate chain against Apple root CA
    verify_apple_certificate_chain(&cert_chain)?;

    // 9. Verify RP ID hash from authData (bundle ID verification)
    if let Some(auth_data_b64) = &attestation_obj.auth_data {
        let auth_data = BASE64.decode(auth_data_b64).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid authData: {}", e))
        })?;

        // AuthData: RP ID Hash (32 bytes) + flags (1 byte) + counter (4 bytes) + ...
        if auth_data.len() < 37 {
            warnings.push("AuthData too short".to_string());
        } else {
            let rp_id_hash = &auth_data[0..32];
            let expected_rp_id_hash = {
                let mut hasher = Sha256::new();
                hasher.update(expected_bundle_id.as_bytes());
                hasher.finalize()
            };

            if rp_id_hash != &expected_rp_id_hash[..] {
                return Ok(AttestationResult {
                    is_valid: false,
                    certificate_hash: cert_hash,
                    bundle_id: expected_bundle_id.to_string(),
                    platform: Platform::IOS,
                    device_trusted: false,
                    verdict: Some("BUNDLE_ID_MISMATCH".to_string()),
                    error: Some("Bundle ID does not match RP ID hash".to_string()),
                    warnings: Some(warnings),
                    verified_at: Utc::now(),
                });
            }
        }
    } else {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Missing authData (required for bundle ID verification)".into(),
        ));
    }

    // 10. Verify App ID from certificate extension
    verify_app_id_from_certificate(&leaf_cert_der, expected_team_id, expected_bundle_id)?;

    // 11. Success - iOS App Attest verified
    Ok(AttestationResult {
        is_valid: true,
        certificate_hash: cert_hash,
        bundle_id: expected_bundle_id.to_string(),
        platform: Platform::IOS,
        device_trusted: true, // Apple App Attest = trusted device
        verdict: Some("APPLE_APPATTEST_VERIFIED".to_string()),
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

pub async fn generate_ios_challenge(
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

    let key = cache_key!(IOS_CHALLENGE_KEY, challenge_hash);

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
    fn test_apple_root_ca_loads() {
        let trust_anchor = webpki::TrustAnchor::try_from_cert_der(APPLE_ROOT_CA_G3);
        assert!(trust_anchor.is_ok());
    }

    #[test]
    fn test_empty_chain_fails() {
        let result = verify_apple_certificate_chain(&[]);
        assert!(result.is_err());
    }
}
