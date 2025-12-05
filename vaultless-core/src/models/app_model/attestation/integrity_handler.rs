use super::dto::*;
use super::types::*;
use crate::error::{Result, VaultlessError};
use serde::{Deserialize, Serialize};

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

pub struct IntegrityConfigHandler {
    pub config: IntegrityConfig,
    pub requirements: IntegrityRequirements,
}

impl IntegrityConfigHandler {
    pub fn new(config_json: &serde_json::Value) -> Result<Self> {
        let config = serde_json::from_value(config_json.clone()).map_err(|e| {
            VaultlessError::Serialization(format!("Failed to parse integrity_config: {}", e))
        })?;

        let requirements = Self::compute_requirements(&config);

        Ok(Self {
            config,
            requirements,
        })
    }

    // Private: Extract platform config without re-parsing
    fn platform_config(&self, platform: Platform) -> Option<&dyn std::any::Any> {
        match platform {
            Platform::IOS => Some(&self.config.ios as &dyn std::any::Any),
            Platform::Android => Some(&self.config.android as &dyn std::any::Any),
            Platform::IoT => Some(&self.config.iot as &dyn std::any::Any),
            Platform::Browser => Some(&self.config.browser as &dyn std::any::Any),
        }
    }

    // Private: Compute requirements once (consolidates duplicated logic)
    fn compute_requirements(config: &IntegrityConfig) -> IntegrityRequirements {
        IntegrityRequirements {
            allow_unauthenticated: config.allow_unauthenticated,
            browser_origin_validation: !config.browser.authorized_origins.is_empty(),
            ios_attestation_required: config.ios.apple_team_id.is_some()
                || !config.ios.allowed_bundle_ids.is_empty(),
            android_attestation_required: config.android.allowed_certificate_sha256.is_some(),
            iot_attestation_required: !config.iot.allowed_certificate_authorities.is_empty()
                || config.iot.require_valid_certificate_expiry,
            ios_reject_untrusted: config.ios.reject_untrusted_device,
            android_reject_untrusted: config.android.reject_untrusted_device,
            iot_reject_untrusted: config.iot.strict_mode,
        }
    }

    pub fn requires_attestation(&self, platform: Platform) -> bool {
        if self.requirements.allow_unauthenticated {
            return false;
        }
        match platform {
            Platform::IOS => self.requirements.ios_attestation_required,
            Platform::Android => self.requirements.android_attestation_required,
            Platform::IoT => self.requirements.iot_attestation_required,
            Platform::Browser => false,
        }
    }

    pub fn allows_unauthenticated(&self) -> bool {
        self.requirements.allow_unauthenticated
    }

    pub fn requires_origin_validation(&self) -> bool {
        self.requirements.browser_origin_validation
    }

    pub fn validate_web_origin(&self, origin: &str) -> Result<()> {
        if !self.requirements.browser_origin_validation {
            return Ok(());
        }
        if self
            .config
            .browser
            .authorized_origins
            .iter()
            .any(|o| o == origin)
        {
            Ok(())
        } else {
            Err(VaultlessError::IntegrityCheckFailed(format!(
                "Origin '{}' is not authorized",
                origin
            )))
        }
    }

    pub fn should_reject_untrusted_device(&self, platform: Platform) -> bool {
        match platform {
            Platform::IOS => self.requirements.ios_reject_untrusted,
            Platform::Android => self.requirements.android_reject_untrusted,
            Platform::IoT => self.requirements.iot_reject_untrusted,
            Platform::Browser => false,
        }
    }

    pub fn get_allowed_bundle_ids(&self, platform: Platform) -> Option<Vec<String>> {
        let bundle_ids = match platform {
            Platform::IOS => &self.config.ios.allowed_bundle_ids,
            Platform::Android => &self.config.android.allowed_package_names,
            Platform::IoT => &self.config.iot.allowed_secure_element_ids,
            Platform::Browser => return None,
        };
        if bundle_ids.is_empty() {
            None
        } else {
            Some(bundle_ids.clone())
        }
    }

    pub fn get_min_version_code(&self, platform: Platform) -> Option<i32> {
        match platform {
            Platform::IOS => self.config.ios.min_version_code,
            Platform::Android => self.config.android.min_version_code,
            Platform::IoT => self.config.iot.min_firmware_version.map(|v| v as i32),
            Platform::Browser => None,
        }
    }

    pub fn validate_bundle_id(&self, platform: Platform, bundle_id: &str) -> Result<()> {
        let allowed_bundles = self.get_allowed_bundle_ids(platform).ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("No allowed bundle IDs configured".to_string())
        if !allowed_bundles.contains(&bundle_id.to_string()) {
            let id_type = match platform {
                Platform::IoT => "Device ID",
                _ => "Bundle ID",
            };
            return Err(VaultlessError::IntegrityCheckFailed(format!(
                "{} '{}' is not in the allowed list",
                id_type, bundle_id
            )));
        }
        Ok(())
    }

    pub fn validate_app_version(&self, platform: Platform, version_code: i32) -> Result<()> {
        if let Some(min_version) = self.get_min_version_code(platform) {
            if version_code < min_version {
                let version_type = match platform {
                    Platform::IoT => "Firmware version",
                    _ => "App version",
                };
                return Err(VaultlessError::IntegrityCheckFailed(format!(
                    "{} {} is below minimum required version {}",
                    version_type, version_code, min_version
                )));
            }
        }
        Ok(())
    }

    pub fn get_android_config(&self) -> &AndroidIntegrityConfig {
        &self.config.android
    }

    pub fn get_ios_config(&self) -> &IosIntegrityConfig {
        &self.config.ios
    }

    pub fn get_iot_config(&self) -> &IoTIntegrityConfig {
        &self.config.iot
    }

    pub fn get_rate_limits(&self) -> &RateLimits {
        &self.config.rate_limits
    }

    pub fn get_integrity_requirements(&self) -> &IntegrityRequirements {
        &self.requirements
    }

    // Also simplify this - attestation rate is same for all platforms
    pub fn get_attestation_rate_limit(&self) -> u32 {
        self.config.rate_limits.max_attestations_per_user_per_hour
    }

    // String clone is cheap here since it's Copy for primitives
    pub fn get_expected_cert_hash(&self, platform: Platform) -> Option<&str> {
        match platform {
            Platform::Android => self.config.android.allowed_certificate_sha256.as_deref(),
            Platform::IoT => self
                .config
                .iot
                .allowed_certificate_authorities
                .first()
                .map(|s| s.as_str()),
            _ => None,
        }
    }
}
