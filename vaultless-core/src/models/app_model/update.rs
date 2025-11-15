use super::dto::*;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use sqlx::QueryBuilder;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use validator::Validate;

impl Application {
    pub async fn update<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        update: UpdateApplication,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // ============= VALIDATE INTEGRITY CONFIG IF PROVIDED =============
        if let Some(ref integrity_config_json) = update.integrity_config {
            // Try to parse and validate the config
            let config: IntegrityConfig = serde_json::from_value(integrity_config_json.clone())
                .map_err(|e| VaultlessError::Validation(format!("Invalid integrity_config JSON: {}", e)))?;

            // Validate each platform's config
            config.web.validate()
                .map_err(|e| VaultlessError::Validation(format!("Invalid web config: {}", e)))?;
            
            config.ios.validate()
                .map_err(|e| VaultlessError::Validation(format!("Invalid iOS config: {}", e)))?;
            
            config.android.validate()
                .map_err(|e| VaultlessError::Validation(format!("Invalid Android config: {}", e)))?;

            tracing::debug!(
                app_id = %update.id,
                "Integrity config validation passed"
            );
        }
        // ============= END VALIDATION =============

        // 1. Initialize QueryBuilder
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
        let mut field_count = 0;

        // 2. Dynamically add fields
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

        // Check if any fields were actually updated
        if field_count == 0 {
            tracing::info!(application_id = %update.id, "Update called with no fields. Skipping database operation.");
            return Self::find_by_id(exec, update.id).await;
        }

        // All updates must update the timestamp
        qb.push(", updated_at = NOW()");

        // 3. Finalize and Execute the Query
        qb.push(" WHERE id = ").push_bind(update.id).push(" RETURNING *");

        let query = qb.build_query_as::<Application>();
        let updated_app = query.fetch_one(exec.clone()).await?;

        // 4. Cache Invalidation (Non-critical)
        if let Some(pool) = redis {
            tracing::info!(application_id = %update.id, "Attempting cache invalidation after application update.");
            let invalidate_result =
                Self::invalidate_auth_cache(&updated_app, exec.clone(), pool).await;

            if let Err(e) = invalidate_result {
                tracing::debug!(application_id = %update.id, "Non-critical: Cache invalidation failed: {:?}", e);
            }
        }

        tracing::info!(
            application_id = %update.id,
            fields_updated = field_count,
            "Application updated successfully"
        );

        Ok(updated_app)
    }
}