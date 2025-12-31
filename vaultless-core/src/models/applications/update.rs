use super::dto::*;
use super::integrity::dto::*;
use crate::error::{Result, VaultlessError};
use crate::models::webhook::WebhookRecord;
use deadpool_redis::Pool as RedisPool;
use serde_json::Value as JsonValue;
use sqlx::{Postgres, QueryBuilder, Transaction};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

const INTEGRITY_PLATFORMS: [&str; 4] = ["browser", "ios", "android", "iot"];

macro_rules! dynamic_update {
    ($sep:ident, $($field:expr => $sql_field:expr),* $(,)?) => {{
        $(
            if let Some(ref value) = $field {
                $sep.push(format!("{} = ", $sql_field)).push_bind(value);
            }
        )*
    }};
}

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
        let integrity_patch_opt = Self::validate_and_serialize_integrity(&update)?;
        let webhooks_to_sync = update.webhooks.clone();

        let has_app_fields = update.name.is_some()
            || update.description.is_some()
            || update.is_active.is_some()
            || update.max_ttl_seconds.is_some()
            || update.is_key_rotation_forced.is_some()
            || update.internal_notes.is_some()
            || integrity_patch_opt.is_some();

        let has_webhooks = webhooks_to_sync.is_some();

        // If no updates at all, return existing app
        if !has_app_fields && !has_webhooks {
            tracing::info!(application_id = %application_id, "No fields to update");
            return Self::find_by_id_and_user_id(exec.as_ref(), application_id, user_id).await;
        }

        // Use transaction if we have webhooks to sync
        if has_webhooks {
            let mut tx = exec.begin().await?;

            // Update application fields if any
            let updated_app = if has_app_fields {
                let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
                Self::build_update_fields(&mut qb, &update, &integrity_patch_opt);
                Self::finalize_update_query(&mut qb, application_id, user_id);

                let app = qb
                    .build_query_as::<Application>()
                    .fetch_one(&mut *tx)
                    .await?;

                if integrity_patch_opt.is_some() {
                    Self::validate_app_meta(&app.app_meta)?;
                }
                app
            } else {
                sqlx::query_as::<_, Application>(
                    "SELECT * FROM applications WHERE id = $1 AND user_id = $2",
                )
                .bind(application_id)
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await?
            };

            // Sync webhooks
            if let Some(webhooks) = webhooks_to_sync {
                WebhookRecord::sync_webhooks(&mut tx, application_id, &webhooks).await?;
            }

            tx.commit().await?;

            if let Some(pool) = redis {
                let exec_clone = exec.clone();
                tokio::spawn(async move {
                    Self::invalidate_caches(exec_clone, pool, application_id).await;
                });
            }

            tracing::info!(application_id = %application_id, "Application updated successfully with webhooks");
            return Ok(updated_app);
        }

        // No webhooks, use simple update
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
        Self::build_update_fields(&mut qb, &update, &integrity_patch_opt);

        if Self::is_empty_update(qb.sql()) {
            tracing::info!(application_id = %application_id, "No fields to update");
            return Self::find_by_id_and_user_id(exec.as_ref(), application_id, user_id).await;
        }

        Self::finalize_update_query(&mut qb, application_id, user_id);

        let updated_app = qb
            .build_query_as::<Application>()
            .fetch_one(exec.as_ref())
            .await?;

        if integrity_patch_opt.is_some() {
            Self::validate_app_meta(&updated_app.app_meta)?;
        }

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
        let webhooks_to_sync = update.webhooks.clone();

        let has_app_fields = update.name.is_some()
            || update.description.is_some()
            || update.is_active.is_some()
            || update.max_ttl_seconds.is_some()
            || update.is_key_rotation_forced.is_some()
            || update.internal_notes.is_some()
            || integrity_patch_opt.is_some();

        // Update application fields
        let updated_app = if has_app_fields {
            let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
            Self::build_update_fields(&mut qb, &update, &integrity_patch_opt);

            if Self::is_empty_update(qb.sql()) {
                sqlx::query_as::<_, Application>(
                    "SELECT * FROM applications WHERE id = $1 AND user_id = $2",
                )
                .bind(application_id)
                .bind(user_id)
                .fetch_one(&mut **tx)
                .await?
            } else {
                Self::finalize_update_query(&mut qb, application_id, user_id);

                let app = qb
                    .build_query_as::<Application>()
                    .fetch_one(&mut **tx)
                    .await?;

                if integrity_patch_opt.is_some() {
                    Self::validate_app_meta(&app.app_meta)?;
                }
                app
            }
        } else {
            sqlx::query_as::<_, Application>(
                "SELECT * FROM applications WHERE id = $1 AND user_id = $2",
            )
            .bind(application_id)
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await?
        };

        // Sync webhooks if provided
        if let Some(webhooks) = webhooks_to_sync {
            WebhookRecord::sync_webhooks(tx, application_id, &webhooks).await?;
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
            Ok(Some(
                serde_json::to_value(cfg)
                    .map_err(|e| VaultlessError::Serialization(e.to_string()))?,
            ))
        } else {
            Ok(None)
        }
    }

    fn build_update_fields<'a>(
        qb: &mut QueryBuilder<'a, Postgres>,
        update: &'a UpdateApplication,
        integrity_patch_opt: &'a Option<JsonValue>,
    ) {
        let mut separated = qb.separated(", ");

        dynamic_update!(
            separated,
            update.name => "name",
            update.description => "description",
            update.is_active => "is_active",
            update.max_ttl_seconds => "max_ttl_seconds",
            update.is_key_rotation_forced => "is_key_rotation_forced",
            update.internal_notes => "internal_notes",
        );

        if let Some(patch) = integrity_patch_opt {
            let wrapped_patch = Self::create_app_meta_patch(patch);

            separated
                .push("app_meta = jsonb_merge_patch(app_meta, ")
                .push_bind(wrapped_patch)
                .push(")");
        }
    }

    fn is_empty_update(sql: &str) -> bool {
        sql.trim_end().ends_with("SET")
            || sql.trim().eq_ignore_ascii_case("UPDATE applications SET")
    }

    fn finalize_update_query(qb: &mut QueryBuilder<'_, Postgres>, application_id: Uuid, user_id: Uuid) {
        qb.push(" , updated_at = NOW() WHERE id = ")
            .push_bind(application_id)
            .push(" AND user_id = ")
            .push_bind(user_id)
            .push(" RETURNING *");
    }

    fn validate_app_meta<T: serde::Serialize>(config: &T) -> Result<()> {
        let config_json = serde_json::to_value(config)
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

        let app_meta = AppMetaData::from_jsonb(&config_json)?;
        let config = app_meta.integrity_config;

        if let Some(ref c) = config.browser {
            c.validate()
                .map_err(|e| VaultlessError::Validation(format!("Browser config invalid: {}", e)))?;
        }
        if let Some(ref c) = config.ios {
            c.validate()
                .map_err(|e| VaultlessError::Validation(format!("iOS config invalid: {}", e)))?;
        }
        if let Some(ref c) = config.android {
            c.validate()
                .map_err(|e| VaultlessError::Validation(format!("Android config invalid: {}", e)))?;
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

    pub async fn batch_update(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        updates: Vec<(Uuid, UpdateApplication)>,
        user_id: Uuid,
    ) -> Result<Vec<Application>> {
        let mut tx = exec.begin().await?;
        let mut results: Vec<Application> = Vec::with_capacity(updates.len());
        let mut updated_ids: Vec<Uuid> = Vec::with_capacity(updates.len());

        for (app_id, update) in updates {
            let app = Self::update_with_tx(&mut tx, update, app_id, user_id).await?;
            updated_ids.push(app_id);
            results.push(app);
        }

        tx.commit().await?;

        if let Some(pool) = redis {
            let exec_clone = exec.clone();
            for app_id in updated_ids {
                let pool_clone = pool.clone();
                let exec_clone2 = exec_clone.clone();
                tokio::spawn(async move {
                    Self::invalidate_caches(exec_clone2, pool_clone, app_id).await;
                });
            }
        }

        Ok(results)
    }
}
