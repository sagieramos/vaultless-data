use super::types::Platform;
use crate::error::{Result, VaultlessError};
use serde_json::Value;

/// Platform-specific configuration extracted from Application.integrity_config
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    pub platform: Platform,
    pub reject_untrusted_device: bool,
    pub min_version_code: Option<i32>,
    pub allowed_bundle_ids: Vec<String>,
}

/// Android-specific configuration
#[derive(Debug, Clone)]
pub struct AndroidConfig {
    pub base: PlatformConfig,
    pub certificate_sha256: String,
    pub google_cloud_project: String,
    pub google_api_key: String,
    pub max_token_age_seconds: u64,
    pub reject_unrecognized_version: bool,
}

/// iOS-specific configuration
#[derive(Debug, Clone)]
pub struct IosConfig {
    pub base: PlatformConfig,
    pub apple_team_id: String,
    pub allowed_certificate_hashes: Vec<String>,
    pub challenge_ttl_seconds: u64,
}

/// IoT-specific configuration
#[derive(Debug, Clone)]
pub struct IotConfig {
    pub base: PlatformConfig,
    pub require_device_certificate: bool,
    pub allowed_certificate_authorities: Vec<String>,
    pub challenge_ttl_seconds: u64,
    pub require_cn_match: bool,
}

// Browser-specific configuration
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    pub allowed_browser: bool,
    pub authorized_origins: Vec<String>,
    pub require_origin_header: bool,
    pub bind_client_to_origin: bool,
    pub require_captcha_on_registration: bool,
    pub captcha_provider: String,
    pub max_clients_per_ip: u32,
    pub max_registrations_per_ip_per_hour: u32,
    pub max_requests_per_client_per_hour: u32,
    pub track_origin_changes: bool,
    pub alert_on_usage_spike: bool,
    pub usage_spike_threshold: f64,
    pub require_proof_of_work: bool,
    pub proof_of_work_difficulty: u32,
}

// =============================================================================
// CONFIG EXTRACTORS
// =============================================================================

pub fn extract_android_config(integrity_config: &Value) -> Result<AndroidConfig> {
    let android = integrity_config
        .get("android")
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing android config".into()))?;

    let certificate_sha256 = android
        .get("allowed_certificate_sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Missing allowed_certificate_sha256".into())
        })?
        .to_string();

    let google_cloud_project = android
        .get("google_cloud_project")
        .and_then(|v| v.as_str())
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing google_cloud_project".into()))?
        .to_string();

    let google_api_key = android
        .get("google_api_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing google_api_key".into()))?
        .to_string();

    Ok(AndroidConfig {
        base: extract_base_config(integrity_config, Platform::Android)?,
        certificate_sha256,
        google_cloud_project,
        google_api_key,
        max_token_age_seconds: android
            .get("max_token_age_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(60),
        reject_unrecognized_version: android
            .get("reject_unrecognized_version")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    })
}

pub fn extract_ios_config(integrity_config: &Value) -> Result<IosConfig> {
    let ios = integrity_config
        .get("ios")
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing ios config".into()))?;

    let apple_team_id = ios
        .get("apple_team_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing apple_team_id".into()))?
        .to_string();

    let allowed_certificate_hashes = ios
        .get("allowed_certificate_hashes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(IosConfig {
        base: extract_base_config(integrity_config, Platform::IOS)?,
        apple_team_id,
        allowed_certificate_hashes,
        challenge_ttl_seconds: ios
            .get("challenge_ttl_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(60),
    })
}

pub fn extract_iot_config(integrity_config: &Value) -> Result<IotConfig> {
    let iot = integrity_config
        .get("iot")
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing iot config".into()))?;

    let require_device_certificate = iot
        .get("require_device_certificate")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !require_device_certificate {
        return Err(VaultlessError::IntegrityCheckFailed(
            "IoT device certificate attestation not enabled".into(),
        ));
    }

    let allowed_certificate_authorities: Vec<_> = iot
        .get("allowed_certificate_authorities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if allowed_certificate_authorities.is_empty() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "No certificate authorities configured for IoT".into(),
        ));
    }

    Ok(IotConfig {
        base: extract_base_config(integrity_config, Platform::IoT)?,
        require_device_certificate,
        allowed_certificate_authorities,
        challenge_ttl_seconds: iot
            .get("challenge_ttl_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(30),
        require_cn_match: iot
            .get("require_cn_match")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    })
}

fn extract_base_config(integrity_config: &Value, platform: Platform) -> Result<PlatformConfig> {
    let platform_config = integrity_config.get(platform.as_str()).ok_or_else(|| {
        VaultlessError::IntegrityCheckFailed(format!("Missing {} config", platform))
    })?;

    let reject_untrusted_device = platform_config
        .get("reject_untrusted_device")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let min_version_code = platform_config
        .get("min_version_code")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i32>().ok());

    let allowed_bundle_ids = platform_config
        .get("allowed_bundle_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(PlatformConfig {
        platform,
        reject_untrusted_device,
        min_version_code,
        allowed_bundle_ids,
    })
}

pub fn extract_browser_config(integrity_config: &serde_json::Value) -> Result<BrowserConfig> {
    let browser = integrity_config
        .get("browser")
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing browser config".into()))?;

    Ok(BrowserConfig {
        allowed_browser: browser
            .get("allowed_browser")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        authorized_origins: browser
            .get("authorized_origins")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        require_origin_header: browser
            .get("require_origin_header")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        bind_client_to_origin: browser
            .get("bind_client_to_origin")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        require_captcha_on_registration: browser
            .get("require_captcha_on_registration")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        captcha_provider: browser
            .get("captcha_provider")
            .and_then(|v| v.as_str())
            .unwrap_or("turnstile")
            .to_string(),
        max_clients_per_ip: browser
            .get("max_clients_per_ip")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as u32,
        max_registrations_per_ip_per_hour: browser
            .get("max_registrations_per_ip_per_hour")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32,
        max_requests_per_client_per_hour: browser
            .get("max_requests_per_client_per_hour")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000) as u32,
        track_origin_changes: browser
            .get("track_origin_changes")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        alert_on_usage_spike: browser
            .get("alert_on_usage_spike")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        usage_spike_threshold: browser
            .get("usage_spike_threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(2.0),
        require_proof_of_work: browser
            .get("require_proof_of_work")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        proof_of_work_difficulty: browser
            .get("proof_of_work_difficulty")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as u32,
    })
}

// =============================================================================
// VALIDATION HELPERS
// =============================================================================

pub fn validate_bundle_id(bundle_id: &str, allowed: &[String]) -> Result<()> {
    if !allowed.is_empty() && !allowed.contains(&bundle_id.to_string()) {
        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "Bundle ID '{}' not in allowed list",
            bundle_id
        )));
    }
    Ok(())
}

pub fn validate_version(app_version: Option<&str>, min_version: Option<i32>) -> Result<()> {
    if let (Some(version_str), Some(min)) = (app_version, min_version)
        && let Ok(version_code) = version_str.parse::<i32>()
        && version_code < min
    {
        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "Version {} below minimum required {}",
            version_code, min
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_android_config() {
        let config = json!({
            "android": {
                "allowed_certificate_sha256": "ABC123",
                "google_cloud_project": "project-123",
                "google_api_key": "key-456",
                "allowed_bundle_ids": ["com.example.app"],
                "reject_untrusted_device": true,
                "max_token_age_seconds": 120
            }
        });

        let android = extract_android_config(&config).unwrap();
        assert_eq!(android.certificate_sha256, "ABC123");
        assert_eq!(android.max_token_age_seconds, 120);
        assert!(android.base.reject_untrusted_device);
    }

    #[test]
    fn test_validate_bundle_id() {
        let allowed = vec!["com.example.app".to_string()];
        assert!(validate_bundle_id("com.example.app", &allowed).is_ok());
        assert!(validate_bundle_id("com.evil.app", &allowed).is_err());
        assert!(validate_bundle_id("anything", &[]).is_ok()); // Empty = allow all
    }
}
