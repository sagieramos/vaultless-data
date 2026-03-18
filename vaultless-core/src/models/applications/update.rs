use super::application::ApplicationFilter;
use super::dto::*;
use super::integrity::dto::*;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use serde_json::Value as JsonValue;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

const INTEGRITY_PLATFORMS: [&str; 4] = ["browser", "ios", "android", "iot"];

impl Application {
    /// Creates the app_meta patch with IntegrityConfig updates and regenerates
    /// platform fingerprint UUIDs for any platforms that are being updated
    fn create_app_meta_patch(integrity_patch: &JsonValue) -> JsonValue {
        let mut fingerprint_updates = serde_json::Map::new();

        // Generate new UUIDs for any platforms that are being updated
        for platform in INTEGRITY_PLATFORMS {
            if integrity_patch.get(platform).is_some() {
                fingerprint_updates.insert(platform.to_string(), serde_json::json!(Uuid::new_v4()));
            }
        }

        serde_json::json!({
            "IntegrityConfig": integrity_patch,
            "PlatformFingerPrint": fingerprint_updates
        })
    }

    pub async fn update(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        update: UpdateApplication,
        application_id: Uuid,
        user_id: Uuid,
    ) -> Result<Application> {
        let mut tx = exec.begin().await?;
        let updated_app = Self::update_with_tx(&mut tx, update, application_id, user_id).await?;
        tx.commit().await?;

        if let Some(pool) = redis {
            tokio::spawn(async move {
                Self::invalidate_caches(exec, pool, application_id).await;
            });
        }

        tracing::info!(application_id = %application_id, "Application updated successfully");
        Ok(updated_app)
    }

    pub async fn update_with_tx(
        tx: &mut Transaction<'_, Postgres>,
        update: UpdateApplication,
        application_id: Uuid,
        user_id: Uuid,
    ) -> Result<Application> {
        let integrity_patch_opt = Self::validate_and_serialize_integrity(&update)?;

        let has_app_fields = update.name.is_some()
            || update.description.is_some()
            || update.is_active.is_some()
            || update.max_ttl_seconds.is_some()
            || update.is_key_rotation_forced.is_some()
            || update.internal_notes.is_some()
            || integrity_patch_opt.is_some()
            || update.webhooks.is_some();

        if !has_app_fields {
            tracing::info!(application_id = %application_id, "No fields to update");
            return Self::find(
                &mut **tx,
                ApplicationFilter::new()
                    .id(application_id)
                    .developer_id(user_id),
            )
            .await;
        }

        let integrity_patch_for_db = integrity_patch_opt
            .as_ref()
            .map(|patch| Self::create_app_meta_patch(patch));

        let webhooks_json = update
            .webhooks
            .map(|wh| serde_json::to_value(wh))
            .transpose()
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

        let updated_app = sqlx::query_as::<_, Application>(
            r#"
            SELECT
                application_id as id,
                developer_id as user_id,
                name,
                description,
                is_active,
                created_at,
                updated_at,
                max_ttl_seconds,
                is_key_rotation_forced,
                deletion_requested_at,
                internal_notes,
                app_meta
            FROM update_application($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(application_id)
        .bind(user_id)
        .bind(&update.name)
        .bind(&update.description)
        .bind(update.is_active)
        .bind(update.max_ttl_seconds)
        .bind(update.is_key_rotation_forced)
        .bind(&update.internal_notes)
        .bind(integrity_patch_for_db)
        .bind(webhooks_json)
        .fetch_one(&mut **tx)
        .await?;

        if integrity_patch_opt.is_some() {
            Self::validate_app_meta(&updated_app.app_meta)?;
        }

        Ok(updated_app)
    }

    fn validate_and_serialize_integrity(update: &UpdateApplication) -> Result<Option<JsonValue>> {
        update
            .validate()
            .map_err(|e| VaultlessError::Validation(format!("Invalid update: {}", e)))?;

        if let Some(ref cfg) = update.integrity_config {
            cfg.validate().map_err(|e| {
                VaultlessError::Validation(format!("Integrity config invalid: {}", e))
            })?;
            Ok(Some(serde_json::to_value(cfg).map_err(|e| {
                VaultlessError::Serialization(e.to_string())
            })?))
        } else {
            Ok(None)
        }
    }

    fn validate_app_meta<T: serde::Serialize>(config: &T) -> Result<()> {
        let config_json = serde_json::to_value(config)
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

        let app_meta = AppMetaData::from_jsonb(&config_json)?;
        let config = app_meta.integrity_config;

        if let Some(ref c) = config.browser {
            c.validate().map_err(|e| {
                VaultlessError::Validation(format!("Browser config invalid: {}", e))
            })?;
        }
        if let Some(ref c) = config.ios {
            c.validate()
                .map_err(|e| VaultlessError::Validation(format!("iOS config invalid: {}", e)))?;
        }
        if let Some(ref c) = config.android {
            c.validate().map_err(|e| {
                VaultlessError::Validation(format!("Android config invalid: {}", e))
            })?;
        }
        if let Some(ref c) = config.iot {
            c.validate()
                .map_err(|e| VaultlessError::Validation(format!("IoT config invalid: {}", e)))?;
        }
        if let Some(ref c) = config.rate_limits {
            c.validate()
                .map_err(|e| VaultlessError::Validation(format!("Rate limits invalid: {}", e)))?;
        }

        Ok(())
    }

    async fn invalidate_caches(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Arc<RedisPool>,
        application_id: Uuid,
    ) {
        super::material_view_helper::trigger_view_refresh_debounced(exec.clone(), redis.clone());

        if let Err(e) = Self::invalidate_auth_cache(application_id, &exec, redis).await {
            tracing::error!(
                application_id = %application_id,
                error = %e,
                "Cache invalidation failed"
            );
        }
    }
}
