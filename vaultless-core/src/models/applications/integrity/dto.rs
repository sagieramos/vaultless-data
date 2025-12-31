use super::Platform;
use crate::error::{Result as VaultlessErrorResult, VaultlessError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use utoipa::ToSchema;

// =============================================================================
// VALIDATOR MOCKS
// =============================================================================
fn validate_origin_list(_origins: &[String]) -> Result<(), VaultlessError> {
    Ok(())
}
fn validate_optional_string_len(_s: &str, _min: usize, _max: usize) -> Result<(), VaultlessError> {
    Ok(())
}
fn validate_bundle_id(_b: &str) -> Result<(), VaultlessError> {
    Ok(())
}
fn validate_sha256(_s: &str) -> Result<(), VaultlessError> {
    Ok(())
}

// =============================================================================
// INTEGRITY CONFIG STRUCTURES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllowedPlatforms {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ios: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iot: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigVersion {
    pub browser: Uuid,
    pub ios: Uuid,
    pub android: Uuid,
    pub iot: Uuid,
}

impl Default for PlatformConfigVersion {
    fn default() -> Self {
        Self {
            browser: Uuid::new_v4(),
            ios: Uuid::new_v4(),
            android: Uuid::new_v4(),
            iot: Uuid::new_v4(),
        }
    }
}

impl PlatformConfigVersion {
    pub const BROWSER_KEY: &'static str = "browser";
    pub const IOS_KEY: &'static str = "ios";
    pub const ANDROID_KEY: &'static str = "android";
    pub const IOT_KEY: &'static str = "iot";

    pub fn new() -> Self {
        Self {
            browser: Uuid::new_v4(),
            ios: Uuid::new_v4(),
            android: Uuid::new_v4(),
            iot: Uuid::new_v4(),
        }
    }

    pub fn from_json(json: &serde_json::Value) -> Self {
        Self {
            browser: json
                .get(Self::BROWSER_KEY)
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or_else(Uuid::new_v4),
            ios: json
                .get(Self::IOS_KEY)
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or_else(Uuid::new_v4),
            android: json
                .get(Self::ANDROID_KEY)
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or_else(Uuid::new_v4),
            iot: json
                .get(Self::IOT_KEY)
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or_else(Uuid::new_v4),
        }
    }

    pub fn get_from_str(&self, platform: &str) -> Option<Uuid> {
        match platform.to_lowercase().as_str() {
            Self::IOS_KEY => Some(self.ios),
            Self::ANDROID_KEY => Some(self.android),
            Self::IOT_KEY => Some(self.iot),
            Self::BROWSER_KEY => Some(self.browser),
            _ => None,
        }
    }

    pub fn get(&self, platform: Platform) -> Uuid {
        match platform {
            Platform::IOS => self.ios,
            Platform::Android => self.android,
            Platform::IoT => self.iot,
            Platform::Browser => self.browser,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppMetaData {
    pub platform_fingerprint: PlatformConfigVersion,
    pub integrity_config: IntegrityConfig,
}
impl AppMetaData {
    pub fn from_jsonb(json: &serde_json::Value) -> VaultlessErrorResult<Self> {
        let pf_json = json
            .get("PlatformFingerPrint")
            .ok_or_else(|| VaultlessError::Serialization("Missing PlatformFingerPrint".into()))?;

        let platform_fingerprint = PlatformConfigVersion::from_json(pf_json);

        let ic_json = json
            .get("IntegrityConfig")
            .ok_or_else(|| VaultlessError::Serialization("Missing IntegrityConfig".into()))?;

        let integrity_config: IntegrityConfig =
            serde_json::from_value(ic_json.clone()).map_err(|e| {
                VaultlessError::Serialization(format!("Failed to parse IntegrityConfig: {}", e))
            })?;

        Ok(Self {
            platform_fingerprint,
            integrity_config,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityConfig {
    #[serde(default)]
    pub allow_unauthenticated: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserIntegrityConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ios: Option<IosIntegrityConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android: Option<AndroidIntegrityConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iot: Option<IoTIntegrityConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limits: Option<RateLimits>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_platforms: Option<AllowedPlatforms>,
}

impl IntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        if let Some(cfg) = &self.browser {
            cfg.validate()?;
        }
        if let Some(cfg) = &self.ios {
            cfg.validate()?;
        }
        if let Some(cfg) = &self.android {
            cfg.validate()?;
        }
        if let Some(cfg) = &self.iot {
            cfg.validate()?;
        }
        if let Some(cfg) = &self.rate_limits {
            cfg.validate()?;
        }
        Ok(())
    }
}

// =============================================================================
// Browser Integrity Config
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserIntegrityConfig {
    pub authorized_origins: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reattestation_days: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_origin_header: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_referer_header: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cors_strict_mode: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_captcha_on_registration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub captcha_provider: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub captcha_site_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub captcha_secret_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind_client_to_origin: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_origin_changes: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_origin_changes_per_client: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_clients_per_ip: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_registrations_per_ip_per_hour: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_requests_per_ip_per_hour: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_on_usage_spike: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_spike_threshold: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_baseline_hours: Option<u64>,
}

impl Default for BrowserIntegrityConfig {
    fn default() -> Self {
        Self {
            authorized_origins: Vec::new(),
            reattestation_days: Some(30),
            require_origin_header: Some(true),
            require_referer_header: Some(true),
            cors_strict_mode: Some(true),
            require_captcha_on_registration: Some(true),
            captcha_provider: Some("turnstile".to_string()),
            captcha_site_key: None,
            captcha_secret_key: None,
            bind_client_to_origin: Some(true),
            track_origin_changes: Some(true),
            max_origin_changes_per_client: Some(3),
            max_clients_per_ip: Some(50),
            max_registrations_per_ip_per_hour: Some(5),
            max_requests_per_ip_per_hour: Some(300),
            alert_on_usage_spike: Some(true),
            usage_spike_threshold: Some(3.0),
            usage_baseline_hours: Some(24),
        }
    }
}

impl BrowserIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        validate_origin_list(&self.authorized_origins)?;

        if let Some(max_clients) = self.max_clients_per_ip {
            if max_clients > 1000 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "max_clients_per_ip cannot exceed 1000".into(),
                ));
            }
        }
        Ok(())
    }

    /// Calculate trust score based on enabled security features and constraints.
    /// Returns a score between 0 and 100, where higher scores indicate stronger security posture.
    pub fn calculate_trust_score(&self) -> u8 {
        let mut score: u16 = 0;

        // Origin validation (max 25 points)
        if !self.authorized_origins.is_empty() {
            score += 10;
        }
        if self.require_origin_header.unwrap_or(false) {
            score += 8;
        }
        if self.require_referer_header.unwrap_or(false) {
            score += 7;
        }

        // CORS and security (max 15 points)
        if self.cors_strict_mode.unwrap_or(false) {
            score += 10;
        }
        if self.bind_client_to_origin.unwrap_or(false) {
            score += 5;
        }

        // CAPTCHA protection (max 20 points)
        if self.require_captcha_on_registration.unwrap_or(false) {
            score += 15;
        }
        if self.captcha_provider.is_some() && self.captcha_secret_key.is_some() {
            score += 5;
        }

        // Rate limiting (max 25 points)
        if self.max_clients_per_ip.is_some() {
            score += 5;
        }
        if self.max_registrations_per_ip_per_hour.is_some() {
            score += 8;
        }
        if self.max_requests_per_ip_per_hour.is_some() {
            score += 7;
        }
        if let Some(max_changes) = self.max_origin_changes_per_client {
            if max_changes <= 3 {
                score += 5;
            }
        }

        // Monitoring and alerts (max 15 points)
        if self.track_origin_changes.unwrap_or(false) {
            score += 8;
        }
        if self.alert_on_usage_spike.unwrap_or(false) {
            score += 7;
        }

        score.min(100) as u8
    }
}

// =============================================================================
// iOS Integrity Config
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IosIntegrityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reattestation_days: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub apple_team_id: Option<String>,

    pub allowed_bundle_ids: Vec<String>,
    pub allowed_certificate_hashes: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version_code: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_untrusted_device: Option<bool>,

    #[serde(skip_serializing, skip_deserializing)]
    attestation_fingerprint: Uuid,
}

impl Default for IosIntegrityConfig {
    fn default() -> Self {
        Self {
            reattestation_days: Some(30),
            apple_team_id: None,
            allowed_bundle_ids: Vec::new(),
            allowed_certificate_hashes: Vec::new(),
            min_version_code: None,
            reject_untrusted_device: Some(false),
            attestation_fingerprint: Uuid::new_v4(),
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

    /// Calculate trust score based on enabled security features and constraints.
    /// Returns a score between 0 and 100, where higher scores indicate stronger security posture.
    pub fn calculate_trust_score(&self) -> u8 {
        let mut score: u16 = 0;

        // App identity verification (max 40 points)
        if !self.allowed_bundle_ids.is_empty() {
            score += 20;
        }
        if self.apple_team_id.is_some() {
            score += 20;
        }

        // Certificate pinning (max 25 points)
        if !self.allowed_certificate_hashes.is_empty() {
            score += 25;
        }

        // Version enforcement (max 15 points)
        if self.min_version_code.is_some() {
            score += 15;
        }

        // Device trust (max 10 points)
        if self.reject_untrusted_device.unwrap_or(false) {
            score += 10;
        }

        // Re-attestation frequency (max 10 points)
        if let Some(days) = self.reattestation_days {
            if days <= 7 {
                score += 10;
            } else if days <= 30 {
                score += 5;
            }
        }

        score.min(100) as u8
    }
}

// =============================================================================
// Android Integrity Config
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AndroidIntegrityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reattestation_days: Option<u32>,

    #[serde(default)]
    pub allowed_certificate_sha256: Vec<String>,

    #[serde(default)]
    pub allowed_package_names: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version_code: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_untrusted_device: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_unrecognized_version: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_unlicensed_app: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_cloud_project: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_token_age_seconds: Option<u64>,
}

impl Default for AndroidIntegrityConfig {
    fn default() -> Self {
        Self {
            reattestation_days: Some(30),
            allowed_certificate_sha256: Vec::new(),
            allowed_package_names: Vec::new(),
            min_version_code: None,
            reject_untrusted_device: Some(false),
            reject_unrecognized_version: Some(true),
            reject_unlicensed_app: Some(false),
            google_cloud_project: None,
            google_api_key: None,
            max_token_age_seconds: Some(60),
        }
    }
}

impl AndroidIntegrityConfig {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        for sha in &self.allowed_certificate_sha256 {
            validate_sha256(sha)?;
        }
        for bundle in &self.allowed_package_names {
            validate_bundle_id(bundle)?;
        }
        Ok(())
    }

    /// Calculate trust score based on enabled security features and constraints.
    /// Returns a score between 0 and 100, where higher scores indicate stronger security posture.
    pub fn calculate_trust_score(&self) -> u8 {
        let mut score: u16 = 0;

        // App identity verification (max 30 points)
        if !self.allowed_package_names.is_empty() {
            score += 15;
        }
        if !self.allowed_certificate_sha256.is_empty() {
            score += 15;
        }

        // Version enforcement (max 15 points)
        if self.min_version_code.is_some() {
            score += 15;
        }

        // Device and app trust (max 30 points)
        if self.reject_untrusted_device.unwrap_or(false) {
            score += 15;
        }
        if self.reject_unrecognized_version.unwrap_or(false) {
            score += 8;
        }
        if self.reject_unlicensed_app.unwrap_or(false) {
            score += 7;
        }

        // Token freshness (max 10 points)
        if let Some(max_age) = self.max_token_age_seconds {
            if max_age <= 60 {
                score += 10;
            } else if max_age <= 300 {
                score += 5;
            }
        }

        // Re-attestation frequency (max 10 points)
        if let Some(days) = self.reattestation_days {
            if days <= 7 {
                score += 10;
            } else if days <= 30 {
                score += 5;
            }
        }

        // Google Play Integrity API setup (max 5 points)
        if self.google_cloud_project.is_some() && self.google_api_key.is_some() {
            score += 5;
        }

        score.min(100) as u8
    }
}

// =============================================================================
// IoT INTEGRITY CONFIGURATION
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IoTIntegrityConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reattestation_days: Option<u32>,

    #[serde(default)]
    pub allowed_certificate_authorities: Vec<String>,

    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub require_valid_certificate_expiry: Option<bool>,

    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub reject_future_certificates: Option<bool>,

    #[serde(default = "default_true", skip_serializing_if = "is_default_true")]
    pub require_cn_match: Option<bool>,

    #[serde(default)]
    pub required_san_fields: Vec<String>,

    #[serde(default)]
    pub allowed_models: Vec<String>,

    #[serde(default)]
    pub allowed_hardware_revisions: Vec<String>,

    #[serde(default)]
    pub allowed_manufacturers: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_firmware_version: Option<i32>,

    #[serde(default)]
    pub allowed_secure_element_ids: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_device_idle_seconds: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_challenge_signature: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_mode: Option<bool>,
}

impl Default for IoTIntegrityConfig {
    fn default() -> Self {
        Self {
            reattestation_days: None,
            allowed_certificate_authorities: vec![],
            require_valid_certificate_expiry: Some(true),
            reject_future_certificates: Some(true),
            require_cn_match: Some(true),
            required_san_fields: vec![],
            allowed_models: vec![],
            allowed_hardware_revisions: vec![],
            allowed_manufacturers: vec![],
            min_firmware_version: None,
            allowed_secure_element_ids: vec![],
            max_device_idle_seconds: None,
            require_challenge_signature: Some(false),
            strict_mode: Some(false),
        }
    }
}

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
        if self.require_valid_certificate_expiry.unwrap_or(false) {
            score += 6;
        }
        if self.reject_future_certificates.unwrap_or(false) {
            score += 6;
        }

        // Device identity (max 25 points)
        if self.require_cn_match.unwrap_or(false) {
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
        if self.require_challenge_signature.unwrap_or(false) {
            score += 15;
        }

        // Strict mode bonus (max 5 points)
        if self.strict_mode.unwrap_or(false) {
            score += 5;
        }

        score.min(100) as u8
    }
}

// =============================================================================
// Rate Limits
// =============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RateLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attestations_per_user_per_hour: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_failed_attempts_before_lockout: Option<u32>,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            max_attestations_per_user_per_hour: Some(50),
            max_failed_attempts_before_lockout: Some(5),
        }
    }
}

impl RateLimits {
    pub fn validate(&self) -> Result<(), VaultlessError> {
        if let Some(limit) = self.max_attestations_per_user_per_hour {
            if limit < 1 || limit > 1000 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "max_attestations_per_user_per_hour out of range (1-1000)".into(),
                ));
            }
        }

        if let Some(limit) = self.max_failed_attempts_before_lockout {
            if limit > 100 {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "max_failed_attempts_before_lockout out of range".into(),
                ));
            }
        }
        Ok(())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn default_true() -> Option<bool> {
    Some(true)
}

fn is_default_true(v: &Option<bool>) -> bool {
    *v == Some(true)
}
