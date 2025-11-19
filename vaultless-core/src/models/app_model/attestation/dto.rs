use serde::{Deserialize, Serialize};
use validator::Validate;

// =============================================================================
// INTEGRITY CONFIG STRUCTURES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrityConfig {
    #[serde(default)]
    pub allow_unauthenticated: bool,

    #[serde(default)]
    pub browser: BrowserIntegrityConfig,

    #[serde(default)]
    pub ios: IosIntegrityConfig,

    #[serde(default)]
    pub android: AndroidIntegrityConfig,

    #[serde(default)]
    pub iot: IoTIntegrityConfig,

    #[serde(default)]
    pub rate_limits: RateLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct BrowserIntegrityConfig {
    // Origin validation
    #[serde(default)]
    #[validate(length(max = 100))]
    pub authorized_origins: Vec<String>,
    
    #[serde(default = "default_true")]
    pub require_origin_header: bool,
    
    #[serde(default = "default_true")]
    pub require_referer_header: bool,
    
    #[serde(default = "default_true")]
    pub cors_strict_mode: bool,
    
    // CAPTCHA configuration
    #[serde(default = "default_true")]
    pub require_captcha_on_registration: bool,
    
    #[serde(default = "default_captcha_provider")]
    pub captcha_provider: String, // "turnstile" | "hcaptcha" | "recaptcha"
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captcha_site_key: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captcha_secret_key: Option<String>,
    
    // Client binding
    #[serde(default = "default_true")]
    pub bind_client_to_origin: bool,
    
    #[serde(default = "default_true")]
    pub track_origin_changes: bool,
    
    #[serde(default = "default_max_origin_changes")]
    pub max_origin_changes_per_client: u32,
    
    // Rate limiting
    #[serde(default = "default_max_clients_per_ip")]
    pub max_clients_per_ip: u32,
    
    #[serde(default = "default_max_registrations_per_hour")]
    pub max_registrations_per_ip_per_hour: u32,
    
    #[serde(default = "default_max_requests_per_hour")]
    pub max_requests_per_ip_per_hour: u32,
    
    // Usage spike detection
    #[serde(default = "default_true")]
    pub alert_on_usage_spike: bool,
    
    #[serde(default = "default_usage_spike_threshold")]
    pub usage_spike_threshold: f64,
    
    #[serde(default = "default_usage_baseline_hours")]
    pub usage_baseline_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct IosIntegrityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 10, max = 10))]
    pub apple_team_id: Option<String>,

    #[serde(default)]
    #[validate(length(max = 50))]
    pub allowed_bundle_ids: Vec<String>,

    /// Optional certificate hash pinning for additional security
    #[serde(default)]
    pub allowed_certificate_hashes: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version_code: Option<i32>,

    #[serde(default)]
    pub reject_untrusted_device: bool,

    /// Challenge TTL in seconds (default: 60)
    #[serde(default = "default_ios_challenge_ttl")]
    pub challenge_ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct AndroidIntegrityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 32, max = 128))]
    pub allowed_certificate_sha256: Option<String>,

    #[serde(default)]
    #[validate(length(max = 50))]
    pub allowed_bundle_ids: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version_code: Option<i32>,

    #[serde(default)]
    pub reject_untrusted_device: bool,

    /// Reject apps with UNRECOGNIZED_VERSION verdict (default: true)
    #[serde(default = "default_true")]
    pub reject_unrecognized_version: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_cloud_project: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_api_key: Option<String>,

    /// Max token age in seconds (default: 60)
    #[serde(default = "default_max_token_age")]
    pub max_token_age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct IoTIntegrityConfig {
    #[serde(default)]
    pub require_device_certificate: bool,

    /// Base64-encoded CA certificates (Ed25519 DER format)
    #[serde(default)]
    pub allowed_certificate_authorities: Vec<String>,

    /// Optional: Restrict to specific device IDs
    #[serde(default)]
    pub allowed_device_ids: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_firmware_version: Option<i32>,

    /// Challenge TTL in seconds (default: 30)
    #[serde(default = "default_iot_challenge_ttl")]
    pub challenge_ttl_seconds: u64,

    /// Require certificate CN to match device_id (default: true)
    #[serde(default = "default_true")]
    pub require_cn_match: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RateLimits {
    /// Max attestation attempts per user per hour (default: 100)
    #[serde(default = "default_max_attestations")]
    pub max_attestations_per_user_per_hour: u32,

    /// Max failed attempts before temporary lockout (default: 5)
    #[serde(default = "default_max_failed")]
    pub max_failed_attempts_before_lockout: u32,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            max_attestations_per_user_per_hour: 100,
            max_failed_attempts_before_lockout: 5,
        }
    }
}

// =============================================================================
// DEFAULT VALUE FUNCTIONS
// =============================================================================

fn default_true() -> bool {
    true
}

fn default_captcha_provider() -> String {
    "turnstile".to_string()
}

fn default_max_origin_changes() -> u32 {
    3
}

fn default_max_clients_per_ip() -> u32 {
    50
}

fn default_max_registrations_per_hour() -> u32 {
    5
}

fn default_max_requests_per_hour() -> u32 {
    300
}

fn default_usage_spike_threshold() -> f64 {
    3.0
}

fn default_usage_baseline_hours() -> u64 {
    24
}

fn default_ios_challenge_ttl() -> u64 {
    60
}

fn default_iot_challenge_ttl() -> u64 {
    30
}

fn default_max_token_age() -> u64 {
    60
}

fn default_max_attestations() -> u32 {
    100
}

fn default_max_failed() -> u32 {
    5
}

// =============================================================================
// UPDATE REQUEST
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateIntegrityConfigRequest {
    #[serde(default)]
    pub allow_unauthenticated: bool,

    #[serde(default)]
    #[validate(nested)]
    pub browser: BrowserIntegrityConfig,

    #[serde(default)]
    #[validate(nested)]
    pub ios: IosIntegrityConfig,

    #[serde(default)]
    #[validate(nested)]
    pub android: AndroidIntegrityConfig,

    #[serde(default)]
    #[validate(nested)]
    pub iot: IoTIntegrityConfig,

    #[serde(default)]
    #[validate(nested)]
    pub rate_limits: RateLimits,
}

// =============================================================================
// INTEGRITY REQUIREMENTS SUMMARY
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrityRequirements {
    pub allow_unauthenticated: bool,
    pub browser_origin_validation: bool,
    pub ios_attestation_required: bool,
    pub android_attestation_required: bool,
    pub iot_attestation_required: bool,
    pub ios_reject_untrusted: bool,
    pub android_reject_untrusted: bool,
    pub iot_reject_untrusted: bool,
}

// =============================================================================
// VALIDATION HELPERS
// =============================================================================

impl UpdateIntegrityConfigRequest {
    /// Validate Android config has required fields if attestation is enabled
    pub fn validate_android_config(&self) -> Result<(), String> {
        if self.android.allowed_certificate_sha256.is_some() {
            // If Android attestation is configured, require Google Cloud credentials
            if self.android.google_cloud_project.is_none() {
                return Err("google_cloud_project required for Android attestation".into());
            }
            if self.android.google_api_key.is_none() {
                return Err("google_api_key required for Android attestation".into());
            }
        }
        Ok(())
    }

    /// Validate iOS config has required fields if attestation is enabled
    pub fn validate_ios_config(&self) -> Result<(), String> {
        if !self.ios.allowed_bundle_ids.is_empty() && self.ios.apple_team_id.is_none() {
            return Err("apple_team_id required when bundle_ids are specified".into());
        }
        Ok(())
    }

    /// Validate IoT config has required fields if attestation is enabled
    pub fn validate_iot_config(&self) -> Result<(), String> {
        if self.iot.require_device_certificate
            && self.iot.allowed_certificate_authorities.is_empty()
        {
            return Err("At least one CA certificate required for IoT attestation".into());
        }
        Ok(())
    }

    /// Perform all custom validations
    pub fn validate_all(&self) -> Result<(), String> {
        self.validate_android_config()?;
        self.validate_ios_config()?;
        self.validate_iot_config()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integrity_config_defaults() {
        let config = IntegrityConfig::default();
        assert!(!config.allow_unauthenticated);
        assert_eq!(config.rate_limits.max_attestations_per_user_per_hour, 100);
        assert_eq!(config.rate_limits.max_failed_attempts_before_lockout, 5);
    }

    #[test]
    fn test_ios_config_validation() {
        let mut request = UpdateIntegrityConfigRequest {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig {
                apple_team_id: None,
                allowed_bundle_ids: vec!["com.example.app".to_string()],
                ..Default::default()
            },
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        };

        // Should fail without team ID
        assert!(request.validate_ios_config().is_err());

        // Should pass with team ID
        request.ios.apple_team_id = Some("ABCD123456".to_string());
        assert!(request.validate_ios_config().is_ok());
    }

    #[test]
    fn test_android_config_validation() {
        let mut request = UpdateIntegrityConfigRequest {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig {
                allowed_certificate_sha256: Some("ABC123".to_string()),
                google_cloud_project: None,
                google_api_key: None,
                ..Default::default()
            },
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        };

        // Should fail without Google Cloud credentials
        assert!(request.validate_android_config().is_err());

        // Should pass with credentials
        request.android.google_cloud_project = Some("project-123".to_string());
        request.android.google_api_key = Some("key-456".to_string());
        assert!(request.validate_android_config().is_ok());
    }

    #[test]
    fn test_iot_config_validation() {
        let mut request = UpdateIntegrityConfigRequest {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig {
                require_device_certificate: true,
                allowed_certificate_authorities: vec![],
                ..Default::default()
            },
            rate_limits: RateLimits::default(),
        };

        // Should fail without CAs
        assert!(request.validate_iot_config().is_err());

        // Should pass with at least one CA
        request.iot.allowed_certificate_authorities = vec!["ca_cert_base64".to_string()];
        assert!(request.validate_iot_config().is_ok());
    }

    #[test]
    fn test_rate_limits_serialization() {
        let limits = RateLimits {
            max_attestations_per_user_per_hour: 50,
            max_failed_attempts_before_lockout: 3,
        };

        let json = serde_json::to_string(&limits).unwrap();
        let deserialized: RateLimits = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.max_attestations_per_user_per_hour, 50);
        assert_eq!(deserialized.max_failed_attempts_before_lockout, 3);
    }
}