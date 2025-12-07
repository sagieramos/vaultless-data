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

/// Configuration for IoT device attestation and integrity verification.
/// Supports enterprise-grade security controls including certificate validation,
/// firmware attestation, hardware root of trust, and runtime security policies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IoTIntegrityConfig {
    #[serde(default)]
    pub allowed_certificate_authorities: Vec<String>,

    #[serde(default = "default_true")]
    pub require_valid_certificate_expiry: bool,

    #[serde(default = "default_true")]
    pub reject_future_certificates: bool,

    // Identity
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

    // Firmware
    #[serde(default)]
    pub min_firmware_version: Option<i32>,

    #[serde(default)]
    pub allowed_firmware_hashes: Vec<String>,

    #[serde(default)]
    pub require_secure_boot: bool,

    #[serde(default)]
    pub require_anti_rollback: bool,

    // Hardware root of trust
    #[serde(default)]
    pub allowed_secure_element_ids: Vec<String>,

    #[serde(default)]
    pub require_hardware_bound_key: bool,

    // Runtime trust
    #[serde(default)]
    pub min_trust_score: Option<u8>,

    #[serde(default)]
    pub max_device_idle_seconds: Option<u64>,

    // Replay protection
    #[serde(default = "default_challenge_ttl")]
    pub challenge_ttl_seconds: u64,

    #[serde(default)]
    pub reject_replayed_signatures: bool,

    // Network
    #[serde(default)]
    pub allowed_ip_ranges: Vec<String>,

    #[serde(default)]
    pub allowed_countries: Vec<String>,

    #[serde(default)]
    pub block_high_risk_countries: bool,

    // Debug
    #[serde(default)]
    pub strict_mode: bool,
}

impl IoTIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        // Certificate authorities
        if self.allowed_certificate_authorities.len() > 5 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 5 certificate authorities allowed".into(),
            ));
        }

        // SAN fields
        if self.required_san_fields.len() > 5 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 5 SAN fields allowed".into(),
            ));
        }

        // Models
        if self.allowed_models.len() > 10 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 10 allowed models".into(),
            ));
        }

        // Hardware revisions
        if self.allowed_hardware_revisions.len() > 10 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 10 allowed hardware revisions".into(),
            ));
        }

        // Manufacturers
        if self.allowed_manufacturers.len() > 10 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 10 allowed manufacturers".into(),
            ));
        }

        // Firmware version
        if let Some(v) = self.min_firmware_version {
            if v < 1 || v > 999_999 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "Firmware version must be between 1 and 999999".into(),
                ));
            }
        }

        // Allowed firmware hashes
        for hash in &self.allowed_firmware_hashes {
            validate_sha256(hash)?;
        }

        // Secure element IDs
        if self.allowed_secure_element_ids.len() > 100 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 100 secure element IDs allowed".into(),
            ));
        }

        // Trust score
        if let Some(score) = self.min_trust_score {
            if score > 100 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "Trust score must be between 0 and 100".into(),
                ));
            }
        }

        // Idle time
        if let Some(seconds) = self.max_device_idle_seconds {
            if seconds < 60 || seconds > 2_592_000 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "Idle time must be between 60 seconds and 30 days".into(),
                ));
            }
        }

        // Challenge TTL
        if self.challenge_ttl_seconds < 30 || self.challenge_ttl_seconds > 86_400 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Challenge TTL must be between 30 and 86,400 seconds".into(),
            ));
        }

        // IP ranges
        if self.allowed_ip_ranges.len() > 20 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 20 allowed IP ranges".into(),
            ));
        }

        // Countries
        if self.allowed_countries.len() > 50 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Maximum 50 allowed countries".into(),
            ));
        }

        Ok(())
    }
}

// =============================================================================
// DEFAULT VALUE HELPERS
// =============================================================================

fn default_true() -> bool {
    true
}

fn default_challenge_ttl() -> u64 {
    300 // 5 minutes
}

// =============================================================================
// IMPLEMENTATION
// =============================================================================

impl Default for IoTIntegrityConfig {
    /// Default configuration with balanced security for most deployments.
    fn default() -> Self {
        Self {
            // Certificate requirements
            allowed_certificate_authorities: vec![],
            require_valid_certificate_expiry: true,
            reject_future_certificates: true,

            // Identity requirements
            require_cn_match: true,
            required_san_fields: vec![],
            allowed_models: vec![],
            allowed_hardware_revisions: vec![],
            allowed_manufacturers: vec![],

            // Firmware attestation
            min_firmware_version: None,
            allowed_firmware_hashes: vec![],
            require_secure_boot: false,
            require_anti_rollback: false,

            // Hardware root of trust
            allowed_secure_element_ids: vec![],
            require_hardware_bound_key: false,

            // Runtime trust
            min_trust_score: None,
            max_device_idle_seconds: None,

            // Replay protection
            challenge_ttl_seconds: 300,
            reject_replayed_signatures: false,

            // Network restrictions
            allowed_ip_ranges: vec![],
            allowed_countries: vec![],
            block_high_risk_countries: false,

            // Debug controls
            strict_mode: false,
        }
    }
}

impl IoTIntegrityConfig {
    /// Create a minimal configuration for development/testing.
    /// ⚠️ NOT SUITABLE FOR PRODUCTION - lacks security controls.
    pub fn development() -> Self {
        Self {
            require_valid_certificate_expiry: false,
            reject_future_certificates: false,
            require_cn_match: false,
            ..Default::default()
        }
    }

    /// Create a high-security configuration for critical infrastructure.
    /// Enforces hardware root of trust, secure boot, and strict validation.
    pub fn high_security() -> Self {
        Self {
            require_valid_certificate_expiry: true,
            reject_future_certificates: true,
            require_cn_match: true,
            require_secure_boot: true,
            require_anti_rollback: true,
            require_hardware_bound_key: true,
            reject_replayed_signatures: true,
            strict_mode: true,
            challenge_ttl_seconds: 180, // 3 minutes
            ..Default::default()
        }
    }

    /// Create a balanced configuration for enterprise IoT deployments.
    /// Good security without requiring specialized hardware.
    pub fn enterprise() -> Self {
        Self {
            require_valid_certificate_expiry: true,
            reject_future_certificates: true,
            require_cn_match: true,
            challenge_ttl_seconds: 300,
            max_device_idle_seconds: Some(86400), // 24 hours
            ..Default::default()
        }
    }

    /// Validate configuration constraints.
    pub fn validate_config(&self) -> Result<(), String> {
        // Validate using validator crate
        self.validate()
            .map_err(|e| format!("Configuration validation failed: {}", e))?;

        // Custom business logic validations
        if self.require_hardware_bound_key && self.allowed_secure_element_ids.is_empty() {
            return Err(
                "require_hardware_bound_key is enabled but no secure element IDs specified. \
                 Either disable hardware binding or provide allowed_secure_element_ids."
                    .to_string(),
            );
        }

        if !self.allowed_firmware_hashes.is_empty() {
            for hash in &self.allowed_firmware_hashes {
                if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(format!(
                        "Invalid firmware hash '{}'. Must be 64 hex characters (SHA-256).",
                        hash
                    ));
                }
            }
        }

        if !self.allowed_certificate_authorities.is_empty() {
            for ca in &self.allowed_certificate_authorities {
                if ca.is_empty() {
                    return Err("Empty certificate authority in allowlist".to_string());
                }
            }
        }

        Ok(())
    }

    /// Check if this is a production-ready configuration.
    pub fn is_production_ready(&self) -> bool {
        self.require_valid_certificate_expiry
            && self.reject_future_certificates
            && self.require_cn_match
            && self.challenge_ttl_seconds <= 600
    }

    /// Get a human-readable security level description.
    pub fn security_level(&self) -> &'static str {
        let score = self.calculate_security_score();
        match score {
            90..=100 => "Critical (High Security)",
            70..=89 => "Enterprise (Strong Security)",
            50..=69 => "Standard (Moderate Security)",
            30..=49 => "Basic (Minimal Security)",
            _ => "Development (Insecure)",
        }
    }

    /// Calculate a security score (0-100) based on enabled features.
    fn calculate_security_score(&self) -> u8 {
        let mut score = 0u8;

        // Certificate validation (30 points)
        if self.require_valid_certificate_expiry {
            score += 10;
        }
        if self.reject_future_certificates {
            score += 5;
        }
        if !self.allowed_certificate_authorities.is_empty() {
            score += 15;
        }

        // Identity validation (20 points)
        if self.require_cn_match {
            score += 10;
        }
        if !self.required_san_fields.is_empty() {
            score += 5;
        }
        if !self.allowed_models.is_empty() || !self.allowed_manufacturers.is_empty() {
            score += 5;
        }

        // Firmware attestation (25 points)
        if self.min_firmware_version.is_some() {
            score += 5;
        }
        if !self.allowed_firmware_hashes.is_empty() {
            score += 10;
        }
        if self.require_secure_boot {
            score += 5;
        }
        if self.require_anti_rollback {
            score += 5;
        }

        // Hardware root of trust (15 points)
        if self.require_hardware_bound_key {
            score += 10;
        }
        if !self.allowed_secure_element_ids.is_empty() {
            score += 5;
        }

        // Runtime security (10 points)
        if self.reject_replayed_signatures {
            score += 5;
        }
        if self.challenge_ttl_seconds <= 300 {
            score += 5;
        }

        // Penalties
        if !self.is_production_ready() {
            score = score.saturating_sub(10);
        }

        score
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
