use super::dto::*;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use sqlx::Postgres;
use std::sync::Arc;
use validator::Validate;

use serde::{Deserialize, Serialize};

/// Request for updating integrity configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct IntegrityConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unauthenticated: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserIntegrityConfigRequest>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ios: Option<IosIntegrityConfigRequest>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub android: Option<AndroidIntegrityConfigRequest>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub iot: Option<IoTIntegrityConfigRequest>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limits: Option<RateLimitsRequest>,
}

// -----------------------------------------------------------------------------
// Browser
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct BrowserIntegrityConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 100))]
    pub authorized_origins: Option<Vec<String>>,

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

// -----------------------------------------------------------------------------
// iOS
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct IosIntegrityConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 10, max = 10))]
    pub apple_team_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50))]
    pub allowed_bundle_ids: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_certificate_hashes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version_code: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_untrusted_device: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_ttl_seconds: Option<u64>,
}

// -----------------------------------------------------------------------------
// Android
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct AndroidIntegrityConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 32, max = 128))]
    pub allowed_certificate_sha256: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50))]
    pub allowed_bundle_ids: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version_code: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_untrusted_device: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reject_unrecognized_version: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_cloud_project: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_token_age_seconds: Option<u64>,
}

// -----------------------------------------------------------------------------
// IoT
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct IoTIntegrityConfigRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_device_certificate: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_certificate_authorities: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_device_ids: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_firmware_version: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_ttl_seconds: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_cn_match: Option<bool>,
}

// -----------------------------------------------------------------------------
// Rate limits
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, Validate, Default)]
pub struct RateLimitsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attestations_per_user_per_hour: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_failed_attempts_before_lockout: Option<u32>,
}

impl Application {
    pub async fn update_integrity_config(
        &self,
        db: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        config: IntegrityConfigRequest,
    ) -> Result<Application> {
        config
            .validate()
            .map_err(|e| VaultlessError::Validation(format!("Invalid integrity config: {}", e)))?;

        // Perform JSON merge at the database level using jsonb_merge_patch
        let config_patch = serde_json::to_value(&config)
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

        let updated_app = sqlx::query_as::<_, Application>(
            r#"
            UPDATE applications
            SET integrity_config = jsonb_merge_patch(integrity_config, $1),
                updated_at = NOW()
            WHERE id = $2
            RETURNING *
            "#,
        )
        .bind(&config_patch)
        .bind(self.id)
        .fetch_one(db.as_ref())
        .await?;

        if let Some(redis_pool) = redis {
            super::material_view_helper::trigger_view_refresh_debounced(
                db.clone(),
                redis_pool.clone(),
            );

            let db_clone = db.clone();
            let redis_clone = Arc::clone(&redis_pool);
            let app_id = self.id;

            tokio::spawn(async move {
                if let Err(e) = Self::invalidate_auth_cache(app_id, &db_clone, redis_clone).await {
                    tracing::error!(
                        "Background cache invalidation failed for app {}: {}",
                        app_id,
                        e
                    );
                }
            });
        }

        tracing::info!(
            app_id = %self.id,
            "Integrity configuration updated successfully"
        );

        Ok(updated_app)
    }
}
