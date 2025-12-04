use super::validators::*;
use crate::error::VaultlessError;
use serde::{Deserialize, Serialize};

// =============================================================================
// INTEGRITY CONFIG STRUCTURES
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrityConfig {
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_unauthenticated: bool,

    #[serde(default, skip_serializing_if = "is_default")]
    pub browser: BrowserIntegrityConfig,

    #[serde(default, skip_serializing_if = "is_default")]
    pub ios: IosIntegrityConfig,

    #[serde(default, skip_serializing_if = "is_default")]
    pub android: AndroidIntegrityConfig,

    #[serde(default, skip_serializing_if = "is_default")]
    pub iot: IoTIntegrityConfig,

    #[serde(default, skip_serializing_if = "is_default")]
    pub rate_limits: RateLimits,
}

impl IntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        self.browser.validate()?;
        self.ios.validate()?;
        self.android.validate()?;
        self.iot.validate()?;
        self.rate_limits.validate()?;
        Ok(())
    }
}

// =============================================================================
// Browser Integrity Config
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrowserIntegrityConfig {
    pub authorized_origins: Vec<String>,
    pub require_origin_header: bool,
    pub require_referer_header: bool,
    pub cors_strict_mode: bool,
    pub require_captcha_on_registration: bool,
    pub captcha_provider: String,
    pub captcha_site_key: Option<String>,
    pub captcha_secret_key: Option<String>,
    pub bind_client_to_origin: bool,
    pub track_origin_changes: bool,
    pub max_origin_changes_per_client: u32,
    pub max_clients_per_ip: u32,
    pub max_registrations_per_ip_per_hour: u32,
    pub max_requests_per_ip_per_hour: u32,
    pub alert_on_usage_spike: bool,
    pub usage_spike_threshold: f64,
    pub usage_baseline_hours: u64,
}

impl Default for BrowserIntegrityConfig {
    fn default() -> Self {
        Self {
            authorized_origins: Vec::new(),
            require_origin_header: true,
            require_referer_header: true,
            cors_strict_mode: true,
            require_captcha_on_registration: true,
            captcha_provider: "turnstile".into(),
            captcha_site_key: None,
            captcha_secret_key: None,
            bind_client_to_origin: true,
            track_origin_changes: true,
            max_origin_changes_per_client: 3,
            max_clients_per_ip: 50,
            max_registrations_per_ip_per_hour: 5,
            max_requests_per_ip_per_hour: 300,
            alert_on_usage_spike: true,
            usage_spike_threshold: 3.0,
            usage_baseline_hours: 24,
        }
    }
}

impl BrowserIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        validate_origin_list(&self.authorized_origins)?;
        if self.max_clients_per_ip > 1000 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "max_clients_per_ip cannot exceed 1000".into(),
            ));
        }
        Ok(())
    }
}

// =============================================================================
// iOS Integrity Config
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IosIntegrityConfig {
    pub apple_team_id: Option<String>,
    pub allowed_bundle_ids: Vec<String>,
    pub allowed_certificate_hashes: Vec<String>,
    pub min_version_code: Option<i32>,
    pub reject_untrusted_device: bool,
    pub challenge_ttl_seconds: u64,
}

impl Default for IosIntegrityConfig {
    fn default() -> Self {
        Self {
            apple_team_id: None,
            allowed_bundle_ids: Vec::new(),
            allowed_certificate_hashes: Vec::new(),
            min_version_code: None,
            reject_untrusted_device: false,
            challenge_ttl_seconds: 60,
        }
    }
}

impl IosIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        if let Some(ref id) = self.apple_team_id {
            validate_optional_string_len(id, 10, 10)?;
        }
        for bundle in &self.allowed_bundle_ids {
            validate_bundle_id(bundle)?;
        }
        for cert in &self.allowed_certificate_hashes {
            validate_sha256(cert)?;
        }
        Ok(())
    }
}

// =============================================================================
// Android Integrity Config
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AndroidIntegrityConfig {
    pub allowed_certificate_sha256: Option<String>,
    pub allowed_bundle_ids: Vec<String>,
    pub min_version_code: Option<i32>,
    pub reject_untrusted_device: bool,
    pub reject_unrecognized_version: bool,
    pub google_cloud_project: Option<String>,
    pub google_api_key: Option<String>,
    pub max_token_age_seconds: u64,
}

impl Default for AndroidIntegrityConfig {
    fn default() -> Self {
        Self {
            allowed_certificate_sha256: None,
            allowed_bundle_ids: Vec::new(),
            min_version_code: None,
            reject_untrusted_device: false,
            reject_unrecognized_version: true,
            google_cloud_project: None,
            google_api_key: None,
            max_token_age_seconds: 60,
        }
    }
}

impl AndroidIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        if let Some(sha) = &self.allowed_certificate_sha256 {
            validate_sha256(sha)?;
        }
        for bundle in &self.allowed_bundle_ids {
            validate_bundle_id(bundle)?;
        }
        Ok(())
    }
}

// =============================================================================
// IoT Integrity Config
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IoTIntegrityConfig {
    pub require_device_certificate: bool,
    pub allowed_certificate_authorities: Vec<String>,
    pub allowed_device_ids: Vec<String>,
    pub min_firmware_version: Option<i32>,
    pub challenge_ttl_seconds: u64,
    pub require_cn_match: bool,
}

impl Default for IoTIntegrityConfig {
    fn default() -> Self {
        Self {
            require_device_certificate: false,
            allowed_certificate_authorities: Vec::new(),
            allowed_device_ids: Vec::new(),
            min_firmware_version: None,
            challenge_ttl_seconds: 30,
            require_cn_match: true,
        }
    }
}

impl IoTIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        for dev_id in &self.allowed_device_ids {
            validate_device_id(dev_id)?;
        }
        for ca in &self.allowed_certificate_authorities {
            validate_ca_name(ca)?;
        }
        Ok(())
    }
}

// =============================================================================
// Rate Limits
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimits {
    pub max_attestations_per_user_per_hour: u32,
    pub max_failed_attempts_before_lockout: u32,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            max_attestations_per_user_per_hour: 50,
            max_failed_attempts_before_lockout: 5,
        }
    }
}

impl RateLimits {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        if self.max_attestations_per_user_per_hour < 1
            || self.max_attestations_per_user_per_hour > 1000
        {
            return Err(VaultlessError::IntegrityCheckFailed(
                "max_attestations_per_user_per_hour out of range".into(),
            ));
        }
        if self.max_failed_attempts_before_lockout > 100 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "max_failed_attempts_before_lockout out of range".into(),
            ));
        }
        Ok(())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================
fn is_true(v: &bool) -> bool {
    *v
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    v == &T::default()
}
