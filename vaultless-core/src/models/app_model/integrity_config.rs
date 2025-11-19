use super::attestation::dto::*;
use super::dto::*;
use crate::error::{Result, VaultlessError};
use crate::models::app_model::attestation::types::Platform;
use deadpool_redis::Pool as RedisPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

impl Application {
    pub async fn update_integrity_config<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        config: UpdateIntegrityConfigRequest,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. Validate the configuration
        config
            .validate()
            .map_err(|e| VaultlessError::Validation(format!("Invalid integrity config: {}", e)))?;

        // 2. Convert to IntegrityConfig struct
        let integrity_config = IntegrityConfig {
            allow_unauthenticated: config.allow_unauthenticated,
            browser: config.browser,
            ios: config.ios,
            android: config.android,
            iot: config.iot,
            rate_limits: config.rate_limits,
        };

        // 3. Serialize to JSON
        let config_json = serde_json::to_value(&integrity_config)
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

        // 4. Update in database
        let updated_app = sqlx::query_as::<_, Application>(
            r#"
            UPDATE applications
            SET integrity_config = $1,
                updated_at = NOW()
            WHERE id = $2
            RETURNING *
            "#,
        )
        .bind(&config_json)
        .bind(app_id)
        .fetch_one(exec.clone())
        .await?;

        // 5. Invalidate cache
        if let Some(pool) = redis {
            let _ = Self::invalidate_auth_cache(&updated_app, exec, pool).await;
        }

        tracing::info!(
            app_id = %app_id,
            "Integrity configuration updated successfully"
        );

        Ok(updated_app)
    }

    /// Get parsed integrity config
    pub fn get_integrity_config(&self) -> Result<IntegrityConfig> {
        serde_json::from_value(self.integrity_config.clone()).map_err(|e| {
            VaultlessError::Serialization(format!("Failed to parse integrity_config: {}", e))
        })
    }

    /// Check if platform attestation is required for a given platform
    pub fn requires_attestation(&self, platform: Platform) -> bool {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return false,
        };

        // If unauthenticated access is allowed, attestation is not required
        if config.allow_unauthenticated {
            return false;
        }

        match platform {
            Platform::IOS => {
                // Attestation required if team ID is configured
                config.ios.apple_team_id.is_some() || !config.ios.allowed_bundle_ids.is_empty()
            }
            Platform::Android => config.android.allowed_certificate_sha256.is_some(),
            Platform::IoT => {
                // IoT attestation required if CAs are configured
                config.iot.require_device_certificate
                    && !config.iot.allowed_certificate_authorities.is_empty()
            }
            Platform::Browser => {
                // Web uses origin validation, not attestation
                false
            }
        }
    }

    /// Check if unauthenticated access is allowed (dev/test mode)
    pub fn allows_unauthenticated(&self) -> bool {
        self.get_integrity_config()
            .map(|c| c.allow_unauthenticated)
            .unwrap_or(false)
    }

    /// Check if web origin validation is required
    pub fn requires_origin_validation(&self) -> bool {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return false,
        };

        !config.browser.authorized_origins.is_empty()
    }

    /// Validate a web origin against configured origins
    pub fn validate_web_origin(&self, origin: &str) -> Result<()> {
        let config = self.get_integrity_config()?;

        if config.browser.authorized_origins.is_empty() {
            // No origins configured, allow all (fail open)
            return Ok(());
        }

        if config.browser.authorized_origins.iter().any(|o| o == origin) {
            Ok(())
        } else {
            Err(VaultlessError::IntegrityCheckFailed(format!(
                "Origin '{}' is not authorized",
                origin
            )))
        }
    }

    /// Get expected certificate hash for a platform
    pub fn get_expected_cert_hash(&self, platform: Platform) -> Option<String> {
        let config = self.get_integrity_config().ok()?;

        match platform {
            Platform::IOS => None, // iOS uses team ID + bundle ID verification
            Platform::Android => config.android.allowed_certificate_sha256.clone(),
            Platform::IoT => {
                // IoT uses CA verification, not direct cert hash
                // But we can return the first CA for compatibility
                config.iot.allowed_certificate_authorities.first().cloned()
            }
            Platform::Browser => None,
        }
    }

    /// Check if untrusted devices should be rejected
    pub fn should_reject_untrusted_device(&self, platform: Platform) -> bool {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return false,
        };

        match platform {
            Platform::IOS => config.ios.reject_untrusted_device,
            Platform::Android => config.android.reject_untrusted_device,
            Platform::IoT => true, // IoT always requires trusted certificates
            Platform::Browser => false,
        }
    }

    /// Get allowed bundle IDs for a platform (or device IDs for IoT)
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

    /// Get minimum version code for a platform
    pub fn get_min_version_code(&self, platform: Platform) -> Option<i32> {
        let config = self.get_integrity_config().ok()?;

        match platform {
            Platform::IOS => config.ios.min_version_code,
            Platform::Android => config.android.min_version_code,
            Platform::IoT => config.iot.min_firmware_version,
            Platform::Browser => None,
        }
    }

    /// Validate bundle ID is allowed
    pub fn validate_bundle_id(&self, platform: Platform, bundle_id: &str) -> Result<()> {
        if let Some(allowed_bundles) = self.get_allowed_bundle_ids(platform) {
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
        }
        // If no bundle IDs configured, allow all
        Ok(())
    }

    /// Validate app version meets minimum requirement
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

    /// Get rate limit configuration
    pub fn get_rate_limits(&self) -> RateLimits {
        self.get_integrity_config()
            .map(|c| c.rate_limits)
            .unwrap_or_default()
    }

    /// Get platform-specific rate limit
    pub fn get_attestation_rate_limit(&self, platform: Platform) -> u32 {
        let rate_limits = self.get_rate_limits();

        match platform {
            Platform::IOS => rate_limits.max_attestations_per_user_per_hour,
            Platform::Android => rate_limits.max_attestations_per_user_per_hour,
            Platform::IoT => rate_limits.max_attestations_per_user_per_hour,
            Platform::Browser => rate_limits.max_attestations_per_user_per_hour,
        }
    }

    /// Get max failed attempts before lockout
    pub fn get_max_failed_attempts(&self) -> u32 {
        self.get_rate_limits().max_failed_attempts_before_lockout
    }

    /// Get Android-specific config
    pub fn get_android_config(&self) -> Result<AndroidIntegrityConfig> {
        let config = self.get_integrity_config()?;
        Ok(config.android)
    }

    /// Get iOS-specific config
    pub fn get_ios_config(&self) -> Result<IosIntegrityConfig> {
        let config = self.get_integrity_config()?;
        Ok(config.ios)
    }

    /// Get IoT-specific config
    pub fn get_iot_config(&self) -> Result<IoTIntegrityConfig> {
        let config = self.get_integrity_config()?;
        Ok(config.iot)
    }

    /// Get a summary of integrity requirements
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
            iot_reject_untrusted: true, // Always true for IoT
        }
    }
}

// Helper function to create default integrity config
impl IntegrityConfig {
    /// Create a new empty integrity config
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a development/testing config (no attestation required)
    pub fn dev_mode() -> Self {
        Self {
            allow_unauthenticated: true,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    /// Create a web-only config
    pub fn browser_only(authorized_origins: Vec<String>) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig {
                authorized_origins,
                ..Default::default()
            },
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    /// Create an iOS-only config
    pub fn ios_only(
        apple_team_id: String,
        bundle_ids: Vec<String>,
        reject_untrusted: bool,
    ) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig {
                apple_team_id: Some(apple_team_id),
                allowed_bundle_ids: bundle_ids,
                allowed_certificate_hashes: vec![],
                min_version_code: None,
                reject_untrusted_device: reject_untrusted,
                challenge_ttl_seconds: 60,
            },
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    /// Create an Android-only config
    pub fn android_only(
        cert_hash: String,
        bundle_ids: Vec<String>,
        google_cloud_project: String,
        google_api_key: String,
        reject_untrusted: bool,
    ) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig {
                allowed_certificate_sha256: Some(cert_hash),
                allowed_bundle_ids: bundle_ids,
                min_version_code: None,
                reject_untrusted_device: reject_untrusted,
                reject_unrecognized_version: true,
                google_cloud_project: Some(google_cloud_project),
                google_api_key: Some(google_api_key),
                max_token_age_seconds: 60,
            },
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    /// Create an IoT-only config
    pub fn iot_only(
        allowed_cas: Vec<String>,
        allowed_device_ids: Vec<String>,
        require_cn_match: bool,
    ) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig {
                require_device_certificate: true,
                allowed_certificate_authorities: allowed_cas,
                allowed_device_ids,
                min_firmware_version: None,
                challenge_ttl_seconds: 30,
                require_cn_match,
            },
            rate_limits: RateLimits::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integrity_config_dev_mode() {
        let config = IntegrityConfig::dev_mode();
        assert!(config.allow_unauthenticated);
    }

    #[test]
    fn test_integrity_config_ios_only() {
        let config = IntegrityConfig::ios_only(
            "ABCD123456".to_string(),
            vec!["com.example.app".to_string()],
            true,
        );
        assert!(!config.allow_unauthenticated);
        assert_eq!(config.ios.apple_team_id, Some("ABCD123456".to_string()));
        assert!(config.ios.reject_untrusted_device);
    }

    #[test]
    fn test_integrity_config_iot_only() {
        let config = IntegrityConfig::iot_only(
            vec!["ca_cert_base64".to_string()],
            vec!["device-123".to_string()],
            true,
        );
        assert!(!config.allow_unauthenticated);
        assert!(config.iot.require_device_certificate);
        assert!(config.iot.require_cn_match);
        assert_eq!(config.iot.challenge_ttl_seconds, 30);
    }
}
