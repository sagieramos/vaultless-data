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
    pub async fn update_integrity_config(
        db: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        config: UpdateIntegrityConfigRequest,
    ) -> Result<Application> {
        config
            .validate()
            .map_err(|e| VaultlessError::Validation(format!("Invalid integrity config: {}", e)))?;

        let integrity_config = IntegrityConfig {
            allow_unauthenticated: config.allow_unauthenticated,
            browser: config.browser,
            ios: config.ios,
            android: config.android,
            iot: config.iot,
            rate_limits: config.rate_limits,
        };

        let config_json = serde_json::to_value(&integrity_config)
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

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
        .fetch_one(&*db)
        .await?;

        if let Some(redis_pool) = redis {
            super::helper::trigger_view_refresh_debounced(db.clone(), redis_pool.clone());

            let db_clone = db.clone();
            let redis_clone = redis_pool.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::invalidate_auth_cache(app_id, &*db_clone, redis_clone).await {
                    tracing::error!(
                        "Background cache invalidation failed for app {}: {}",
                        app_id,
                        e
                    );
                }
            });
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
