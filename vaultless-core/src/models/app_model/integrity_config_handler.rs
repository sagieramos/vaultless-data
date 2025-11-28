use super::attestation::dto::*;
use super::attestation::types::*;
use super::dto::*;
use crate::error::{Result, VaultlessError};
pub struct IntegrityConfigHandler<'a> {
    config: &'a serde_json::Value,
}

impl<'a> IntegrityConfigHandler<'a> {
    pub fn new(config: &'a serde_json::Value) -> Self {
        Self { config }
    }

    pub fn get_integrity_config(&self) -> Result<IntegrityConfig> {
        serde_json::from_value(self.config.clone()).map_err(|e| {
            VaultlessError::Serialization(format!("Failed to parse integrity_config: {}", e))
        })
    }

    pub fn requires_attestation(&self, platform: Platform) -> bool {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return false,
        };

        if config.allow_unauthenticated {
            return false;
        }

        match platform {
            Platform::IOS => {
                config.ios.apple_team_id.is_some() || !config.ios.allowed_bundle_ids.is_empty()
            }
            Platform::Android => config.android.allowed_certificate_sha256.is_some(),
            Platform::IoT => {
                config.iot.require_device_certificate
                    && !config.iot.allowed_certificate_authorities.is_empty()
            }
            Platform::Browser => false,
        }
    }

    pub fn allows_unauthenticated(&self) -> bool {
        self.get_integrity_config()
            .map(|c| c.allow_unauthenticated)
            .unwrap_or(false)
    }

    pub fn requires_origin_validation(&self) -> bool {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return false,
        };

        !config.browser.authorized_origins.is_empty()
    }

    pub fn validate_web_origin(&self, origin: &str) -> Result<()> {
        let config = self.get_integrity_config()?;

        if config.browser.authorized_origins.is_empty() {
            return Ok(());
        }

        if config
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

    pub fn get_expected_cert_hash(&self, platform: Platform) -> Option<String> {
        let config = self.get_integrity_config().ok()?;

        match platform {
            Platform::IOS => None,
            Platform::Android => config.android.allowed_certificate_sha256.clone(),
            Platform::IoT => config.iot.allowed_certificate_authorities.first().cloned(),
            Platform::Browser => None,
        }
    }

    pub fn should_reject_untrusted_device(&self, platform: Platform) -> bool {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return false,
        };

        match platform {
            Platform::IOS => config.ios.reject_untrusted_device,
            Platform::Android => config.android.reject_untrusted_device,
            Platform::IoT => true,
            Platform::Browser => false,
        }
    }

    pub fn get_allowed_bundle_ids(&self, platform: Platform) -> Option<Vec<String>> {
        let config = self.get_integrity_config().ok()?;

        let bundle_ids = match platform {
            Platform::IOS => &config.ios.allowed_bundle_ids,
            Platform::Android => &config.android.allowed_bundle_ids,
            Platform::IoT => &config.iot.allowed_device_ids,
            Platform::Browser => return None,
        };

        if bundle_ids.is_empty() {
            None
        } else {
            Some(bundle_ids.clone())
        }
    }

    pub fn get_min_version_code(&self, platform: Platform) -> Option<i32> {
        let config = self.get_integrity_config().ok()?;

        match platform {
            Platform::IOS => config.ios.min_version_code,
            Platform::Android => config.android.min_version_code,
            Platform::IoT => config.iot.min_firmware_version,
            Platform::Browser => None,
        }
    }

    pub fn validate_bundle_id(&self, platform: Platform, bundle_id: &str) -> Result<()> {
        if let Some(allowed_bundles) = self.get_allowed_bundle_ids(platform)
            && !allowed_bundles.contains(&bundle_id.to_string())
        {
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
        if let Some(min_version) = self.get_min_version_code(platform)
            && version_code < min_version
        {
            let version_type = match platform {
                Platform::IoT => "Firmware version",
                _ => "App version",
            };
            return Err(VaultlessError::IntegrityCheckFailed(format!(
                "{} {} is below minimum required version {}",
                version_type, version_code, min_version
            )));
        }
        Ok(())
    }

    pub fn get_rate_limits(&self) -> RateLimits {
        self.get_integrity_config()
            .map(|c| c.rate_limits)
            .unwrap_or_default()
    }

    pub fn get_attestation_rate_limit(&self, platform: Platform) -> u32 {
        let rate_limits = self.get_rate_limits();

        match platform {
            Platform::IOS => rate_limits.max_attestations_per_user_per_hour,
            Platform::Android => rate_limits.max_attestations_per_user_per_hour,
            Platform::IoT => rate_limits.max_attestations_per_user_per_hour,
            Platform::Browser => rate_limits.max_attestations_per_user_per_hour,
        }
    }

    pub fn get_max_failed_attempts(&self) -> u32 {
        self.get_rate_limits().max_failed_attempts_before_lockout
    }

    pub fn get_android_config(&self) -> Result<AndroidIntegrityConfig> {
        let config = self.get_integrity_config()?;
        Ok(config.android)
    }

    pub fn get_ios_config(&self) -> Result<IosIntegrityConfig> {
        let config = self.get_integrity_config()?;
        Ok(config.ios)
    }

    pub fn get_iot_config(&self) -> Result<IoTIntegrityConfig> {
        let config = self.get_integrity_config()?;
        Ok(config.iot)
    }

    pub fn get_integrity_requirements(&self) -> IntegrityRequirements {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return IntegrityRequirements::default(),
        };

        IntegrityRequirements {
            allow_unauthenticated: config.allow_unauthenticated,
            browser_origin_validation: !config.browser.authorized_origins.is_empty(),
            ios_attestation_required: config.ios.apple_team_id.is_some()
                || !config.ios.allowed_bundle_ids.is_empty(),
            android_attestation_required: config.android.allowed_certificate_sha256.is_some(),
            iot_attestation_required: config.iot.require_device_certificate
                && !config.iot.allowed_certificate_authorities.is_empty(),
            ios_reject_untrusted: config.ios.reject_untrusted_device,
            android_reject_untrusted: config.android.reject_untrusted_device,
            iot_reject_untrusted: true,
        }
    }
}

// Then both Application and ApplicationKeyView use references:
impl Application {
    pub fn integrity(&self) -> IntegrityConfigHandler<'_> {
        IntegrityConfigHandler::new(&self.integrity_config)
    }
}

impl ApplicationKeyView {
    pub fn integrity(&self) -> IntegrityConfigHandler<'_> {
        IntegrityConfigHandler::new(&self.app_integrity_config)
    }
}

// Usage:
/* auth_config.integrity().requires_attestation(Platform::IOS);
application.integrity().get_integrity_config()?; */
