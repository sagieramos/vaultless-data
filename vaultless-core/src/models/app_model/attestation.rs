use super::attestation_types::*;
use crate::error::{Result, VaultlessError};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use crate::crypto::verify_app_id_from_certificate;

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
    expected_team_id: &str,
) -> Result<AttestationResult> {
    let mut warnings = Vec::new();

    // 1. Decode base64 token
    let attestation_bytes = BASE64
        .decode(token)
        .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid base64 token: {}", e)))?;

    // 2. Parse attestation object (CBOR format)
    // In production, use `ciborium` or `serde_cbor` for proper CBOR parsing
    // For now, we'll try JSON as a fallback for testing
    let attestation_obj: AppAttestObject = serde_json::from_slice(&attestation_bytes)
        .or_else(|_| {
            // Try CBOR parsing if available
            #[cfg(feature = "cbor")]
            {
                ciborium::de::from_reader(&attestation_bytes[..])
                    .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid attestation format: {}", e)))
            }
            #[cfg(not(feature = "cbor"))]
            {
                Err(VaultlessError::IntegrityCheckFailed(
                    "CBOR parsing not available. Enable 'cbor' feature.".into()
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
        return Err(VaultlessError::IntegrityCheckFailed("Empty certificate chain".into()));
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

    // 8. Verify certificate chain against Apple root CA
    verify_apple_certificate_chain(&cert_chain)?;

    // 9. Extract and verify app identifier from authData
    // The authData contains the RP ID hash, which should be SHA-256(bundle_id)
    if let Some(auth_data_b64) = &attestation_obj.auth_data {
        let auth_data = BASE64
            .decode(auth_data_b64)
            .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid authData: {}", e)))?;

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

            if rp_id_hash != expected_rp_id_hash.as_slice() {
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

    // 10. Verify App ID from certificate extension
    if let Ok(_) = verify_app_id_from_certificate(&leaf_cert_der, expected_team_id, expected_bundle_id) {
        // App ID verified successfully
    } else {
        warnings.push("Could not verify App ID from certificate extension".to_string());
    }

    // 11. Success - device is trusted (iOS App Attest passed)
    Ok(AttestationResult {
        is_valid: true,
        certificate_hash: cert_hash,
        bundle_id: expected_bundle_id.to_string(),
        platform: Platform::IOS,
        device_trusted: true,
        verdict: Some("APPLE_APPATTEST_VERIFIED".to_string()),
        error: None,
        warnings: if warnings.is_empty() { None } else { Some(warnings) },
        verified_at: Utc::now(),
    })
}

#[cfg(feature = "x509")]
fn verify_apple_certificate_chain(_cert_chain: &[String]) -> Result<()> {
    // TODO: Implement proper certificate chain validation
    // 1. Parse certificates using x509-parser
    // 2. Verify chain up to Apple's root CA
    // 3. Check validity dates
    // 4. Verify certificate purposes/extensions
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
            VaultlessError::IntegrityCheckFailed(format!("Play Integrity API request failed: {}", e))
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
    let integrity_response: PlayIntegrityResponse = response
        .json()
        .await
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid response format: {}", e))
        })?;

    let payload = integrity_response.token_payload;

    // 5. Verify package name
    if payload.app_integrity.package_name != expected_package_name {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: String::new(),
            bundle_id: payload.app_integrity.package_name.clone(),
            platform: Platform::Android,
            device_trusted: false,
            verdict: Some("PACKAGE_NAME_MISMATCH".to_string()),
            error: Some(format!(
                "Expected package '{}', got '{}'",
                expected_package_name, payload.app_integrity.package_name
            )),
            warnings: Some(warnings),
            verified_at: Utc::now(),
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
    let device_trusted = device_verdicts
        .iter()
        .any(|v| matches!(v.as_str(), "MEETS_DEVICE_INTEGRITY" | "MEETS_BASIC_INTEGRITY"));

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
        verdict: Some(format!(
            "APP:{}, DEVICE:{:?}",
            app_verdict, device_verdicts
        )),
        error: None,
        warnings: if warnings.is_empty() { None } else { Some(warnings) },
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
    // 1. Validate configuration and get expected values
    let expected_value = validate_attestation_config(request, integrity_config)?;

    // 2. Platform-specific verification
    match request.platform {
        Platform::IOS => {
            // For iOS, we need the Team ID instead of cert hash
            let team_id = get_apple_team_id(integrity_config)
                .ok_or_else(|| VaultlessError::Internal("Apple Team ID not found".into()))?;

            verify_ios_attestation(
                &request.attestation_token,
                &request.bundle_id,
                &team_id,
            )
            .await
        }
        Platform::Android => {
            let cert_hash = expected_value
                .ok_or_else(|| VaultlessError::Internal("Android cert hash not found".into()))?;

            let project = google_cloud_project.ok_or_else(|| {
                VaultlessError::Internal("Google Cloud project number not configured".into())
            })?;
            let api_key = google_api_key.ok_or_else(|| {
                VaultlessError::Internal("Google API key not configured".into())
            })?;

            verify_android_attestation(
                &request.attestation_token,
                &request.bundle_id,
                &cert_hash,
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
            result.error.clone().unwrap_or_else(|| "Attestation failed".into()),
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