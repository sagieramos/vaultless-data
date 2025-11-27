use super::attestation::dto::IntegrityConfig;
use super::dto::*;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use sqlx::{Postgres, QueryBuilder};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

impl Application {
    pub async fn update(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        update: UpdateApplication,
        application_id: Uuid,
        user_id: Uuid,
    ) -> Result<Application> {
        // ================= VALIDATE INTEGRITY CONFIG =================
        if let Some(ref integrity_config_json) = update.integrity_config {
            let config: IntegrityConfig = serde_json::from_value(integrity_config_json.clone())
                .map_err(|e| {
                    VaultlessError::Validation(format!("Invalid integrity_config JSON: {}", e))
                })?;

            config
                .browser
                .validate()
                .map_err(|e| VaultlessError::Validation(format!("Invalid web config: {}", e)))?;
            config
                .ios
                .validate()
                .map_err(|e| VaultlessError::Validation(format!("Invalid iOS config: {}", e)))?;
            config.android.validate().map_err(|e| {
                VaultlessError::Validation(format!("Invalid Android config: {}", e))
            })?;

            tracing::debug!(app_id = %application_id, "Integrity config validation passed");
        }
        // ============================================================

        // ================= DYNAMIC QUERY BUILDING ==================
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
        let mut field_count = 0;

        if let Some(name) = &update.name {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("name = ").push_bind(name);
            field_count += 1;
        }
        if let Some(description) = &update.description {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("description = ").push_bind(description);
            field_count += 1;
        }
        if let Some(is_active) = &update.is_active {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("is_active = ").push_bind(is_active);
            field_count += 1;
        }
        if let Some(max_ttl_seconds) = &update.max_ttl_seconds {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("max_ttl_seconds = ").push_bind(max_ttl_seconds);
            field_count += 1;
        }
        if let Some(is_key_rotation_forced) = &update.is_key_rotation_forced {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("is_key_rotation_forced = ")
                .push_bind(is_key_rotation_forced);
            field_count += 1;
        }
        if let Some(internal_notes) = &update.internal_notes {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("internal_notes = ").push_bind(internal_notes);
            field_count += 1;
        }
        if let Some(integrity_config) = &update.integrity_config {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("integrity_config = ").push_bind(integrity_config);
            field_count += 1;
        }

        if field_count == 0 {
            tracing::info!(application_id = %application_id, "No fields to update.");
            return Self::find_by_id_and_user_id(&*exec, application_id, user_id).await;
        }

        qb.push(", updated_at = NOW()");

        // ================= SECURITY: enforce user_id =================
        qb.push(" WHERE id = ")
            .push_bind(application_id)
            .push(" AND user_id = ")
            .push_bind(user_id)
            .push(" RETURNING *");

        let query = qb.build_query_as::<Application>();
        let updated_app = query.fetch_one(&*exec).await?;

        // ================= CACHE INVALIDATION =================
        if let Some(pool) = redis {
            if let Err(e) = Self::invalidate_auth_cache(application_id, &*exec, pool).await {
                tracing::debug!(application_id = %application_id, "Cache invalidation failed: {:?}", e);
            }
        }

        tracing::info!(application_id = %application_id, fields_updated = field_count, "Application updated successfully");

        Ok(updated_app)
    }
}
