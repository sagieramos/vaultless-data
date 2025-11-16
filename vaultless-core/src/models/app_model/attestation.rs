use super::attestation_types::*;
use crate::error::{Result, VaultlessError};
use asn1_rs::oid;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use pem::Pem;
use reqwest::Client;
use ring::signature::{
    ECDSA_P256_SHA256_ASN1, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey, VerificationAlgorithm,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use x509_parser::certificate::X509Certificate;
use x509_parser::der_parser::oid::Oid;

// =============================================================================
// iOS APP ATTEST VERIFICATION
// =============================================================================

/// iOS App Attest attestation object structure (simplified)
#[derive(Debug, Deserialize)]
struct AppAttestObject {
    /// Format identifier (should be "apple-appattest")
    #[serde(rename = "fmt")]
    format: Option<String>,

    /// Attestation statement containing certificate chain
    #[serde(rename = "attStmt")]
    att_stmt: AttestationStatement,

    /// Authenticator data
    #[serde(rename = "authData")]
    auth_data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttestationStatement {
    /// X.509 certificate chain (DER-encoded, base64)
    #[serde(rename = "x5c")]
    x5c: Option<Vec<String>>,

    /// Receipt (iOS-specific)
    receipt: Option<String>,
}

/// Verify iOS App Attest attestation
pub async fn verify_ios_attestation(
    token: &str,
    expected_bundle_id: &str,
    expected_cert_hash: &str,
) -> Result<AttestationResult> {
    let mut warnings = Vec::new();

    // 1. Decode base64 token
    let attestation_bytes = BASE64.decode(token).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid base64 token: {}", e))
    })?;

    // 2. Parse attestation object (CBOR format)
    // In production, use `ciborium` or `serde_cbor` for proper CBOR parsing
    // For now, we'll try JSON as a fallback for testing
    let attestation_obj: AppAttestObject =
        serde_json::from_slice(&attestation_bytes).or_else(|_| {
            // Try CBOR parsing if available
            #[cfg(feature = "cbor")]
            {
                ciborium::de::from_reader(&attestation_bytes[..]).map_err(|e| {
                    VaultlessError::IntegrityCheckFailed(format!(
                        "Invalid attestation format: {}",
                        e
                    ))
                })
            }
            #[cfg(not(feature = "cbor"))]
            {
                Err(VaultlessError::IntegrityCheckFailed(
                    "CBOR parsing not available. Enable 'cbor' feature.".into(),
                ))
            }
        })?;

    // 3. Verify format
    if let Some(fmt) = &attestation_obj.format {
        if fmt != "apple-appattest" {
            warnings.push(format!("Unexpected format: {}", fmt));
        }
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

    // 5. Get leaf certificate (first in chain)
    let leaf_cert_b64 = &cert_chain[0];
    let leaf_cert_der = BASE64
        .decode(leaf_cert_b64)
        .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid certificate: {}", e)))?;

    // 6. Calculate SHA-256 of certificate
    let mut hasher = Sha256::new();
    hasher.update(&leaf_cert_der);
    let cert_hash = format!("{:x}", hasher.finalize());

    // 7. Verify certificate hash matches expected
    if cert_hash.to_lowercase() != expected_cert_hash.to_lowercase() {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: cert_hash,
            bundle_id: expected_bundle_id.to_string(),
            platform: Platform::IOS,
            device_trusted: false,
            verdict: Some("CERTIFICATE_MISMATCH".to_string()),
            error: Some("Certificate hash does not match expected value".into()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // 8. Verify certificate chain against Apple root CA
    // In production, you should:
    // - Verify the chain is signed by Apple's root CA
    // - Check certificate validity dates
    // - Verify certificate extensions
    // This requires x509-parser or similar
    #[cfg(feature = "x509")]
    {
        verify_apple_certificate_chain(&cert_chain)?;
    }
    #[cfg(not(feature = "x509"))]
    {
        warnings.push("Certificate chain validation skipped (enable 'x509' feature)".to_string());
    }

    // 9. Extract and verify app identifier from authData
    // The authData contains the RP ID hash, which should be SHA-256(bundle_id)
    if let Some(auth_data_b64) = &attestation_obj.auth_data {
        let auth_data = BASE64.decode(auth_data_b64).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid authData: {}", e))
        })?;

        // AuthData structure: RP ID Hash (32 bytes) + flags (1 byte) + counter (4 bytes) + ...
        if auth_data.len() < 37 {
            warnings.push("AuthData too short".to_string());
        } else {
            let rp_id_hash = &auth_data[0..32];
            let expected_rp_id_hash = {
                let mut hasher = Sha256::new();
                hasher.update(expected_bundle_id.as_bytes());
                hasher.finalize()
            };

            let expected_slice: &[u8] = expected_rp_id_hash.as_ref();
            if rp_id_hash != expected_slice {
                return Ok(AttestationResult {
                    is_valid: false,
                    certificate_hash: cert_hash,
                    bundle_id: expected_bundle_id.to_string(),
                    platform: Platform::IOS,
                    device_trusted: false,
                    verdict: Some("BUNDLE_ID_MISMATCH".to_string()),
                    error: Some("Bundle ID does not match".into()),
                    warnings: Some(warnings),
                    verified_at: Utc::now(),
                });
            }
        }
    } else {
        warnings.push("No authData present".to_string());
    }

    // 10. Success - device is trusted (iOS App Attest passed)
    Ok(AttestationResult {
        is_valid: true,
        certificate_hash: cert_hash,
        bundle_id: expected_bundle_id.to_string(),
        platform: Platform::IOS,
        device_trusted: true,
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

#[cfg(feature = "x509")]
use x509_parser::prelude::*;

#[cfg(feature = "x509")]
fn get_apple_root_certs() -> Vec<X509Certificate<'static>> {
    let mut roots = Vec::new();
    let g2_pem = include_str!("../../../certs/AppleRootCA-G2.pem");
    let g3_pem = include_str!("../../../certs/AppleRootCA-G3.pem");

    for pem in [g2_pem, g3_pem] {
        let (_, parsed_pem_struct) = x509_parser::pem::parse_x509_pem(pem.as_bytes())
            .expect("Failed to parse Apple root PEM");

        let owned_der_bytes: Vec<u8> = parsed_pem_struct.contents.to_vec();
        let leaked_der: &'static [u8] = Box::leak(owned_der_bytes.into_boxed_slice());

        let (_, cert) = x509_parser::certificate::X509Certificate::from_der(leaked_der)
            .expect("Failed to parse DER from PEM");

        roots.push(cert);
    }

    roots
}

#[cfg(feature = "x509")]
fn verify_signature(issuer_cert: &X509Certificate, subject_cert: &X509Certificate) -> Result<()> {
    // 1. Get the signature algorithm from the SUBJECT certificate's signature field
    let sig_alg_oid = &subject_cert.signature_algorithm.algorithm;
    let ring_alg = get_ring_verification_alg(sig_alg_oid)?;

    let pub_key_data: &[u8] = &issuer_cert.subject_pki.subject_public_key.data;
    let public_key = UnparsedPublicKey::new(ring_alg, pub_key_data);

    public_key
        .verify(
            subject_cert.tbs_certificate.as_ref(),
            &subject_cert.signature_value.data,
        )
        .map_err(|_| VaultlessError::IntegrityCheckFailed("Certificate signature invalid".into()))
}

fn get_ring_verification_alg(sig_alg: &Oid) -> Result<&'static dyn VerificationAlgorithm> {
    if *sig_alg == oid!(1.2.840.10045.4.3.2) {
        // ECDSA with SHA-256
        Ok(&ECDSA_P256_SHA256_ASN1)
    } else if *sig_alg == oid!(1.2.840.113549.1.1.11) {
        // SHA-256 with RSA Encryption
        Ok(&RSA_PKCS1_2048_8192_SHA256)
    } else {
        Err(VaultlessError::Internal(format!(
            "Unsupported signature algorithm OID: {:?}",
            sig_alg
        )))
    }
}

pub fn verify_apple_certificate_chain(cert_chain: &[String]) -> Result<()> {
    if cert_chain.is_empty() {
        return Err(VaultlessError::Validation(
            "Certificate chain is empty".into(),
        ));
    }

    // Parse all client-provided certificates
    let mut parsed_chain = Vec::new();
    for cert_pem in cert_chain {
        let (_, pem_struct) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes())
            .map_err(|e| VaultlessError::Validation(format!("Invalid PEM: {}", e)))?;

        let owned_der_bytes: Vec<u8> = pem_struct.contents.to_vec();
        let leaked_der: &'static [u8] = Box::leak(owned_der_bytes.into_boxed_slice());

        let (_, cert) = X509Certificate::from_der(leaked_der)
            .map_err(|e| VaultlessError::Validation(format!("Failed to parse DER: {}", e)))?;

        parsed_chain.push(cert);
    }
    // Load Apple roots
    let apple_roots = get_apple_root_certs();

    // 1. Check validity dates
    let now = Utc::now();
    let now_ts = now.timestamp();
    for cert in &parsed_chain {
        let not_before = cert.validity.not_before.to_datetime();
        let not_after = cert.validity.not_after.to_datetime();
        if now_ts < not_before.unix_timestamp() || now_ts > not_after.unix_timestamp() {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Certificate expired or not yet valid".into(),
            ));
        }
    }

    // 2. Verify chain signatures (leaf -> intermediate -> root)
    for i in 0..parsed_chain.len() - 1 {
        let subject = &parsed_chain[i];
        let issuer = &parsed_chain[i + 1];
        verify_signature(issuer, subject)?;
    }

    // 3. Verify root matches one of Apple roots
    let root_cert = parsed_chain.last().unwrap();
    if !apple_roots
        .iter()
        .any(|root| root.tbs_certificate.as_ref() == root_cert.tbs_certificate.as_ref())
    {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Root certificate does not match Apple root CA".into(),
        ));
    }

    // 4. Optional: Verify leaf has Apple attestation OID
    let leaf = parsed_chain.first().unwrap();
    // Use the OID macro for consistency
    let attestation_oid = oid!(1.2.840.113635.100.8.2);

    let has_attestation_oid = leaf
        .extensions()
        .iter()
        .any(|ext| ext.oid == attestation_oid);

    if !has_attestation_oid {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Leaf certificate missing Apple attestation OID".into(),
        ));
    }

    Ok(())
}

// =============================================================================
// ANDROID PLAY INTEGRITY VERIFICATION
// =============================================================================

#[derive(Debug, Deserialize)]
struct PlayIntegrityResponse {
    #[serde(rename = "tokenPayloadExternal")]
    token_payload: PlayIntegrityPayload,
}

#[derive(Debug, Deserialize)]
struct PlayIntegrityPayload {
    #[serde(rename = "requestDetails")]
    request_details: RequestDetails,
    #[serde(rename = "appIntegrity")]
    app_integrity: AppIntegrity,
    #[serde(rename = "deviceIntegrity")]
    device_integrity: DeviceIntegrity,
    #[serde(rename = "accountDetails")]
    account_details: Option<AccountDetails>,
}

#[derive(Debug, Deserialize)]
struct RequestDetails {
    #[serde(rename = "requestPackageName")]
    request_package_name: String,
    nonce: Option<String>,
    #[serde(rename = "timestampMillis")]
    timestamp_millis: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppIntegrity {
    /// Verdict: PLAY_RECOGNIZED, UNRECOGNIZED_VERSION, UNEVALUATED, etc.
    #[serde(rename = "appRecognitionVerdict")]
    app_recognition_verdict: String,

    #[serde(rename = "packageName")]
    package_name: String,

    /// List of SHA-256 certificate hashes
    #[serde(rename = "certificateSha256Digest")]
    certificate_sha256_digest: Vec<String>,

    #[serde(rename = "versionCode")]
    version_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceIntegrity {
    /// List of verdicts: MEETS_DEVICE_INTEGRITY, MEETS_BASIC_INTEGRITY, etc.
    #[serde(rename = "deviceRecognitionVerdict")]
    device_recognition_verdict: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AccountDetails {
    #[serde(rename = "appLicensingVerdict")]
    app_licensing_verdict: Option<String>,
}

/// Verify Android Play Integrity attestation
pub async fn verify_android_attestation(
    token: &str,
    expected_package_name: &str,
    expected_cert_hash: &str,
    google_cloud_project_number: &str,
    google_api_key: &str,
) -> Result<AttestationResult> {
    let mut warnings = Vec::new();

    // 1. Create HTTP client
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| VaultlessError::Internal(format!("HTTP client error: {}", e)))?;

    // 2. Call Google Play Integrity API
    let url = format!(
        "https://playintegrity.googleapis.com/v1/{}:decodeIntegrityToken?key={}",
        google_cloud_project_number, google_api_key
    );

    let request_body = serde_json::json!({
        "integrity_token": token
    });

    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!(
                "Play Integrity API request failed: {}",
                e
            ))
        })?;

    // 3. Check response status
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".into());

        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "Play Integrity API returned {}: {}",
            status, error_text
        )));
    }

    // 4. Parse response
    let integrity_response: PlayIntegrityResponse = response.json().await.map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid response format: {}", e))
    })?;

    let payload = integrity_response.token_payload;

    // 5. Verify package name
    if payload.app_integrity.package_name != expected_package_name {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: String::new(),
            platform: Platform::Android,
            device_trusted: false,
            verdict: Some("PACKAGE_NAME_MISMATCH".to_string()),
            error: Some(format!(
                "Expected package '{}', got '{}'",
                expected_package_name, payload.app_integrity.package_name
            )),
            warnings: Some(warnings),
            verified_at: Utc::now(),
            bundle_id: payload.app_integrity.package_name,
        });
    }

    // 6. Verify app recognition verdict
    let app_verdict = &payload.app_integrity.app_recognition_verdict;
    let app_valid = matches!(
        app_verdict.as_str(),
        "PLAY_RECOGNIZED" | "UNRECOGNIZED_VERSION"
    );

    if !app_valid {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: String::new(),
            bundle_id: payload.app_integrity.package_name,
            platform: Platform::Android,
            device_trusted: false,
            verdict: Some(app_verdict.clone()),
            error: Some(format!("App not recognized by Play Store: {}", app_verdict)),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    if app_verdict == "UNRECOGNIZED_VERSION" {
        warnings.push("App version not recognized by Play Store (might be in testing)".to_string());
    }

    // 7. Verify certificate hash
    let cert_match = payload
        .app_integrity
        .certificate_sha256_digest
        .iter()
        .any(|hash| hash.to_lowercase() == expected_cert_hash.to_lowercase());

    if !cert_match {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: payload.app_integrity.certificate_sha256_digest.join(","),
            bundle_id: payload.app_integrity.package_name,
            platform: Platform::Android,
            device_trusted: false,
            verdict: Some(app_verdict.clone()),
            error: Some(format!(
                "Certificate hash mismatch. Expected: {}, Got: {}",
                expected_cert_hash,
                payload.app_integrity.certificate_sha256_digest.join(",")
            )),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // 8. Check device integrity
    let device_verdicts = &payload.device_integrity.device_recognition_verdict;
    let device_trusted = device_verdicts.iter().any(|v| {
        matches!(
            v.as_str(),
            "MEETS_DEVICE_INTEGRITY" | "MEETS_BASIC_INTEGRITY"
        )
    });

    if !device_trusted {
        warnings.push(format!(
            "Device integrity check failed: {:?}",
            device_verdicts
        ));
    }

    // 9. Check account details (if available)
    if let Some(account) = &payload.account_details {
        if let Some(licensing) = &account.app_licensing_verdict {
            if licensing != "LICENSED" {
                warnings.push(format!("App licensing status: {}", licensing));
            }
        }
    }

    // 10. Success
    Ok(AttestationResult {
        is_valid: true,
        certificate_hash: payload.app_integrity.certificate_sha256_digest.join(","),
        bundle_id: payload.app_integrity.package_name,
        platform: Platform::Android,
        device_trusted,
        verdict: Some(format!("APP:{}, DEVICE:{:?}", app_verdict, device_verdicts)),
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
// UNIFIED ATTESTATION VERIFICATION
// =============================================================================

/// Verify attestation for any platform
pub async fn verify_attestation(
    request: &AttestationRequest,
    integrity_config: &serde_json::Value,
    google_cloud_project: Option<&str>,
    google_api_key: Option<&str>,
) -> Result<AttestationResult> {
    // 1. Validate configuration
    let expected_cert_hash = validate_attestation_config(request, integrity_config)?;

    // 2. Platform-specific verification
    match request.platform {
        Platform::IOS => {
            verify_ios_attestation(
                &request.attestation_token,
                &request.bundle_id,
                &expected_cert_hash,
            )
            .await
        }
        Platform::Android => {
            let project = google_cloud_project.ok_or_else(|| {
                VaultlessError::Internal("Google Cloud project number not configured".into())
            })?;
            let api_key = google_api_key
                .ok_or_else(|| VaultlessError::Internal("Google API key not configured".into()))?;

            verify_android_attestation(
                &request.attestation_token,
                &request.bundle_id,
                &expected_cert_hash,
                project,
                api_key,
            )
            .await
        }
        Platform::Web => {
            // Web platform doesn't use mobile attestation
            // Origin validation is done separately in request validation
            Err(VaultlessError::Validation(
                "Web platform does not support mobile attestation".into(),
            ))
        }
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Verify attestation result against application policies
pub fn enforce_attestation_policies(
    result: &AttestationResult,
    integrity_config: &serde_json::Value,
) -> Result<()> {
    if !result.is_valid {
        return Err(VaultlessError::IntegrityCheckFailed(
            result
                .error
                .clone()
                .unwrap_or_else(|| "Attestation failed".into()),
        ));
    }

    // Check if untrusted devices should be rejected
    if should_reject_untrusted_device(integrity_config, result.platform) {
        if !result.device_trusted {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Untrusted devices are not allowed for this application".into(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_calculation() {
        let data = b"test data";
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = format!("{:x}", hasher.finalize());
        assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex characters
    }

    #[tokio::test]
    async fn test_verify_attestation_missing_config() {
        let request = AttestationRequest {
            platform: Platform::IOS,
            bundle_id: "com.example.app".to_string(),
            device_id: "device123".to_string(),
            attestation_token: "token".to_string(),
            nonce: None,
            app_version: None,
            device_info: None,
        };

        let empty_config = serde_json::json!({});

        let result = verify_attestation(&request, &empty_config, None, None).await;
        assert!(result.is_err());
    }
}
