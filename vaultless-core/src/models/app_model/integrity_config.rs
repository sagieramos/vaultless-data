use super::attestation_types::Platform;
use super::dto::*;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

impl Application {
    /// Update integrity configuration with validation
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
            web: config.web,
            ios: config.ios,
            android: config.android,
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

        match platform {
            Platform::IOS => {
                // Attestation required if certificate hash is configured
                config.ios.allowed_certificate_sha256.is_some()
            }
            Platform::Android => config.android.allowed_certificate_sha256.is_some(),
            Platform::Web => {
                // Web uses origin validation, not attestation
                false
            }
        }
    }

    /// Check if web origin validation is required
    pub fn requires_origin_validation(&self) -> bool {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return false,
        };

        !config.web.authorized_origins.is_empty()
    }

    /// Validate a web origin against configured origins
    pub fn validate_web_origin(&self, origin: &str) -> Result<()> {
        let config = self.get_integrity_config()?;

        if config.web.authorized_origins.is_empty() {
            // No origins configured, allow all (fail open)
            return Ok(());
        }

        if config.web.authorized_origins.iter().any(|o| o == origin) {
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
            Platform::IOS => config.ios.allowed_certificate_sha256.clone(),
            Platform::Android => config.android.allowed_certificate_sha256.clone(),
            Platform::Web => None,
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
            Platform::Web => false,
        }
    }

    /// Get allowed bundle IDs for a platform
    pub fn get_allowed_bundle_ids(&self, platform: Platform) -> Option<Vec<String>> {
        let config = self.get_integrity_config().ok()?;

        let bundle_ids = match platform {
            Platform::IOS => &config.ios.allowed_bundle_ids,
            Platform::Android => &config.android.allowed_bundle_ids,
            Platform::Web => return None,
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
            Platform::Web => None,
        }
    }

    /// Validate bundle ID is allowed
    pub fn validate_bundle_id(&self, platform: Platform, bundle_id: &str) -> Result<()> {
        if let Some(allowed_bundles) = self.get_allowed_bundle_ids(platform) {
            if !allowed_bundles.contains(&bundle_id.to_string()) {
                return Err(VaultlessError::IntegrityCheckFailed(format!(
                    "Bundle ID '{}' is not in the allowed list",
                    bundle_id
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
                return Err(VaultlessError::IntegrityCheckFailed(format!(
                    "App version {} is below minimum required version {}",
                    version_code, min_version
                )));
            }
        }
        Ok(())
    }

    /// Get a summary of integrity requirements
    pub fn get_integrity_requirements(&self) -> IntegrityRequirements {
        let config = match self.get_integrity_config() {
            Ok(c) => c,
            Err(_) => return IntegrityRequirements::default(),
        };

        IntegrityRequirements {
            web_origin_validation: !config.web.authorized_origins.is_empty(),
            ios_attestation_required: config.ios.allowed_certificate_sha256.is_some(),
            android_attestation_required: config.android.allowed_certificate_sha256.is_some(),
            ios_reject_untrusted: config.ios.reject_untrusted_device,
            android_reject_untrusted: config.android.reject_untrusted_device,
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
            allow_unauthenticated: true, // ← NEW
            web: WebIntegrityConfig::default(),
            ios: MobileIntegrityConfig::default(),
            android: MobileIntegrityConfig::default(),
        }
    }

    /// Create a web-only config
    pub fn web_only(authorized_origins: Vec<String>) -> Self {
        Self {
            allow_unauthenticated: false, // ← ADDED
            web: WebIntegrityConfig { authorized_origins },
            ios: MobileIntegrityConfig::default(),
            android: MobileIntegrityConfig::default(),
        }
    }

    /// Create an iOS-only config
    pub fn ios_only(cert_hash: String, bundle_ids: Vec<String>, reject_untrusted: bool) -> Self {
        Self {
            allow_unauthenticated: false, // ← ADDED
            web: WebIntegrityConfig::default(),
            ios: MobileIntegrityConfig {
                allowed_certificate_sha256: Some(cert_hash),
                allowed_bundle_ids: bundle_ids,
                min_version_code: None,
                reject_untrusted_device: reject_untrusted,
            },
            android: MobileIntegrityConfig::default(),
        }
    }

    /// Create an Android-only config
    pub fn android_only(
        cert_hash: String,
        bundle_ids: Vec<String>,
        reject_untrusted: bool,
    ) -> Self {
        Self {
            allow_unauthenticated: false, // ← ADDED
            web: WebIntegrityConfig::default(),
            ios: MobileIntegrityConfig::default(),
            android: MobileIntegrityConfig {
                allowed_certificate_sha256: Some(cert_hash),
                allowed_bundle_ids: bundle_ids,
                min_version_code: None,
                reject_untrusted_device: reject_untrusted,
            },
        }
    }
}
