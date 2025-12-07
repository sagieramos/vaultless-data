use super::validators::*;
use crate::error::VaultlessError;
use serde::{Deserialize, Serialize};
use validator::Validate;

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
}

impl Default for IosIntegrityConfig {
    fn default() -> Self {
        Self {
            apple_team_id: None,
            allowed_bundle_ids: Vec::new(),
            allowed_certificate_hashes: Vec::new(),
            min_version_code: None,
            reject_untrusted_device: false,
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
    pub allowed_package_names: Vec<String>,
    pub min_version_code: Option<i32>,

    // Device / app trust enforcement
    pub reject_untrusted_device: bool,
    pub reject_unrecognized_version: bool,

    // Licensing enforcement
    pub reject_unlicensed_app: bool,

    // Google API config (optional for online mode)
    pub google_cloud_project: Option<String>,
    pub google_api_key: Option<String>,

    // Token freshness
    pub max_token_age_seconds: u64,
}

impl Default for AndroidIntegrityConfig {
    fn default() -> Self {
        Self {
            allowed_certificate_sha256: None,
            allowed_package_names: Vec::new(),
            min_version_code: None,

            reject_untrusted_device: false,
            reject_unrecognized_version: true,

            reject_unlicensed_app: false,

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
        for bundle in &self.allowed_package_names {
            validate_bundle_id(bundle)?;
        }
        Ok(())
    }
}
// =============================================================================
// IoT INTEGRITY CONFIGURATION
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IoTIntegrityConfig {
    // -------------------------------------------------------------------------
    // Certificate trust
    // -------------------------------------------------------------------------
    #[serde(default)]
    pub allowed_certificate_authorities: Vec<String>,

    #[serde(default = "default_true")]
    pub require_valid_certificate_expiry: bool,

    #[serde(default = "default_true")]
    pub reject_future_certificates: bool,

    // -------------------------------------------------------------------------
    // Device identity
    // -------------------------------------------------------------------------
    #[serde(default = "default_true")]
    pub require_cn_match: bool,

    #[serde(default)]
    pub required_san_fields: Vec<String>,

    #[serde(default)]
    pub allowed_models: Vec<String>,

    #[serde(default)]
    pub allowed_hardware_revisions: Vec<String>,

    #[serde(default)]
    pub allowed_manufacturers: Vec<String>,

    // -------------------------------------------------------------------------
    // Firmware trust
    // -------------------------------------------------------------------------
    #[serde(default)]
    pub min_firmware_version: Option<i32>,

    // -------------------------------------------------------------------------
    // Hardware root of trust
    // -------------------------------------------------------------------------
    #[serde(default)]
    pub allowed_secure_element_ids: Vec<String>,

    // -------------------------------------------------------------------------
    // Runtime trust
    // -------------------------------------------------------------------------
    #[serde(default)]
    pub max_device_idle_seconds: Option<u64>,

    // -------------------------------------------------------------------------
    // Replay protection
    // -------------------------------------------------------------------------
    #[serde(default)]
    pub require_challenge_signature: bool,

    // -------------------------------------------------------------------------
    // Behavior
    // -------------------------------------------------------------------------
    #[serde(default)]
    pub strict_mode: bool,
}

// =============================================================================
// DEFAULT HELPERS
// =============================================================================

fn default_true() -> bool {
    true
}

// =============================================================================
// VALIDATION
// =============================================================================

impl IoTIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        if self.allowed_certificate_authorities.len() > 5 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 5 certificate authorities allowed".into(),
            ));
        }

        if self.required_san_fields.len() > 5 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 5 SAN fields allowed".into(),
            ));
        }

        if self.allowed_models.len() > 10 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 10 allowed models".into(),
            ));
        }

        if self.allowed_hardware_revisions.len() > 10 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 10 hardware revisions allowed".into(),
            ));
        }

        if self.allowed_manufacturers.len() > 10 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 10 manufacturers allowed".into(),
            ));
        }

        if let Some(v) = self.min_firmware_version {
            if v < 1 || v > 999_999 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "Firmware version must be between 1 and 999999".into(),
                ));
            }
        }

        if self.allowed_secure_element_ids.len() > 100 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 100 secure element IDs allowed".into(),
            ));
        }

        if let Some(seconds) = self.max_device_idle_seconds {
            if seconds < 60 || seconds > 2_592_000 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "Idle time must be between 60 seconds and 30 days".into(),
                ));
            }
        }

        Ok(())
    }

    /// Calculate trust score based on enabled security features and constraints.
    /// Returns a score between 0 and 100, where higher scores indicate stronger security posture.
    pub fn calculate_trust_score(&self) -> u8 {
        let mut score: u16 = 0;

        // Certificate trust (max 20 points)
        if !self.allowed_certificate_authorities.is_empty() {
            score += 8;
        }
        if self.require_valid_certificate_expiry {
            score += 6;
        }
        if self.reject_future_certificates {
            score += 6;
        }

        // Device identity (max 25 points)
        if self.require_cn_match {
            score += 5;
        }
        if !self.required_san_fields.is_empty() {
            score += 8;
        }
        if !self.allowed_models.is_empty() {
            score += 4;
        }
        if !self.allowed_hardware_revisions.is_empty() {
            score += 4;
        }
        if !self.allowed_manufacturers.is_empty() {
            score += 4;
        }

        // Firmware trust (max 10 points)
        if self.min_firmware_version.is_some() {
            score += 10;
        }

        // Hardware root of trust (max 15 points)
        if !self.allowed_secure_element_ids.is_empty() {
            score += 15;
        }

        // Runtime trust (max 10 points)
        if self.max_device_idle_seconds.is_some() {
            score += 10;
        }

        // Challenge signature (max 15 points)
        if self.require_challenge_signature {
            score += 15;
        }

        // Strict mode bonus (max 5 points)
        if self.strict_mode {
            score += 5;
        }

        // Cap at 100
        score.min(100) as u8
    }
}

// =============================================================================
// DEFAULT IMPLEMENTATION
// =============================================================================

impl Default for IoTIntegrityConfig {
    fn default() -> Self {
        Self {
            allowed_certificate_authorities: vec![],
            require_valid_certificate_expiry: true,
            reject_future_certificates: true,

            require_cn_match: true,
            required_san_fields: vec![],
            allowed_models: vec![],
            allowed_hardware_revisions: vec![],
            allowed_manufacturers: vec![],

            min_firmware_version: None,

            allowed_secure_element_ids: vec![],

            max_device_idle_seconds: None,

            require_challenge_signature: false,

            strict_mode: false,
        }
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
