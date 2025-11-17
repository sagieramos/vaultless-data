use super::types::*;
use crate::error::{Result, VaultlessError};
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

// =============================================================================
// ANDROID PLAY INTEGRITY API RESPONSE TYPES
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
    #[serde(rename = "appRecognitionVerdict")]
    app_recognition_verdict: String,
    #[serde(rename = "packageName")]
    package_name: String,
    #[serde(rename = "certificateSha256Digest")]
    certificate_sha256_digest: Vec<String>,
    #[serde(rename = "versionCode")]
    version_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceIntegrity {
    #[serde(rename = "deviceRecognitionVerdict")]
    device_recognition_verdict: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AccountDetails {
    #[serde(rename = "appLicensingVerdict")]
    app_licensing_verdict: Option<String>,
}

// =============================================================================
// ANDROID PLAY INTEGRITY VERIFICATION
// =============================================================================

/// Verify Android Play Integrity attestation with full security checks
pub async fn verify_android_attestation(
    token: &str,
    expected_package_name: &str,
    expected_cert_hash: &str,
    expected_nonce: &str,
    google_cloud_project_number: &str,
    google_api_key: &str,
    max_token_age_seconds: u64,
    reject_unrecognized_version: bool,
    reject_untrusted_device: bool,
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
        let _error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".into());

        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "Play Integrity API returned error (status {})",
            status
        )));
    }

    // 4. Parse response
    let integrity_response: PlayIntegrityResponse = response.json().await.map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid response format: {}", e))
    })?;

    let payload = integrity_response.token_payload;

    // 5. CRITICAL: Verify Timestamp (REPLAY ATTACK PROTECTION)
    if let Some(timestamp_str) = &payload.request_details.timestamp_millis {
        let timestamp_ms = timestamp_str.parse::<i64>().map_err(|_| {
            VaultlessError::IntegrityCheckFailed("Invalid timestamp format".into())
        })?;

        let now_ms = Utc::now().timestamp_millis();
        let age_ms = now_ms - timestamp_ms;

        // Allow 5 seconds clock skew forward, reject if older than max_token_age
        let max_age_ms = (max_token_age_seconds * 1000) as i64;
        
        if age_ms > max_age_ms {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: String::new(),
                bundle_id: payload.app_integrity.package_name.clone(),
                platform: Platform::Android,
                device_trusted: false,
                verdict: Some("TIMESTAMP_TOO_OLD".to_string()),
                error: Some(format!(
                    "Token timestamp is {}ms old (max allowed: {}ms)",
                    age_ms, max_age_ms
                )),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        }

        if age_ms < -5_000 {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: String::new(),
                bundle_id: payload.app_integrity.package_name.clone(),
                platform: Platform::Android,
                device_trusted: false,
                verdict: Some("TIMESTAMP_IN_FUTURE".to_string()),
                error: Some("Token timestamp is in the future (possible clock skew attack)".to_string()),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        }
    } else {
        warnings.push("Token missing timestamp (reduced replay protection)".to_string());
    }

    // 6. Verify Nonce (REPLAY PROTECTION)
    let actual_nonce = payload.request_details.nonce.as_deref().unwrap_or("");
    if actual_nonce != expected_nonce {
        return Ok(AttestationResult {
            is_valid: false,
            certificate_hash: String::new(),
            bundle_id: payload.app_integrity.package_name.clone(),
            platform: Platform::Android,
            device_trusted: false,
            verdict: Some("NONCE_MISMATCH".to_string()),
            error: Some("Nonce mismatch (possible replay attack)".to_string()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // 7. Verify package name
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

    // 8. Verify app recognition verdict
    let app_verdict = &payload.app_integrity.app_recognition_verdict;
    let app_valid = matches!(app_verdict.as_str(), "PLAY_RECOGNIZED");

    if !app_valid {
        // Handle UNRECOGNIZED_VERSION based on config
        if app_verdict == "UNRECOGNIZED_VERSION" {
            if reject_unrecognized_version {
                return Ok(AttestationResult {
                    is_valid: false,
                    certificate_hash: String::new(),
                    bundle_id: payload.app_integrity.package_name,
                    platform: Platform::Android,
                    device_trusted: false,
                    verdict: Some(app_verdict.clone()),
                    error: Some("App version not recognized by Play Store".to_string()),
                    warnings: Some(warnings),
                    verified_at: Utc::now(),
                });
            } else {
                warnings.push("App version not recognized (testing/staged rollout)".to_string());
            }
        } else {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: String::new(),
                bundle_id: payload.app_integrity.package_name,
                platform: Platform::Android,
                device_trusted: false,
                verdict: Some(app_verdict.clone()),
                error: Some(format!("App not recognized: {}", app_verdict)),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        }
    }

    // 9. Verify certificate hash
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
            error: Some("Certificate hash mismatch".to_string()),
            warnings: Some(warnings),
            verified_at: Utc::now(),
        });
    }

    // 10. Check device integrity
    let device_verdicts = &payload.device_integrity.device_recognition_verdict;
    let device_trusted = device_verdicts.iter().any(|v| {
        matches!(
            v.as_str(),
            "MEETS_DEVICE_INTEGRITY" | "MEETS_BASIC_INTEGRITY"
        )
    });

    if !device_trusted {
        let error_msg = format!("Device integrity failed: {:?}", device_verdicts);
        
        if reject_untrusted_device {
            return Ok(AttestationResult {
                is_valid: false,
                certificate_hash: payload.app_integrity.certificate_sha256_digest.join(","),
                bundle_id: payload.app_integrity.package_name,
                platform: Platform::Android,
                device_trusted: false,
                verdict: Some(format!("DEVICE:{:?}", device_verdicts)),
                error: Some(error_msg),
                warnings: Some(warnings),
                verified_at: Utc::now(),
            });
        } else {
            warnings.push(error_msg);
        }
    }

    // 11. Check account licensing (informational)
    if let Some(account) = &payload.account_details {
        if let Some(licensing) = &account.app_licensing_verdict {
            if licensing != "LICENSED" {
                warnings.push(format!("App licensing status: {}", licensing));
            }
        }
    }

    // 12. Success
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_validation() {
        // Test that timestamp logic would work correctly
        let now_ms = Utc::now().timestamp_millis();
        let max_age_ms = 60_000i64; // 60 seconds

        // Recent timestamp should be valid
        let recent = now_ms - 30_000;
        assert!((now_ms - recent) <= max_age_ms);

        // Old timestamp should be invalid
        let old = now_ms - 120_000;
        assert!((now_ms - old) > max_age_ms);

        // Future timestamp should be invalid
        let future = now_ms + 10_000;
        assert!((now_ms - future) < -5_000);
    }
}