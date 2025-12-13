use super::attestation::dto::*;
use super::dto::*;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use serde_json::Value as JsonValue;
use sqlx::{Postgres, QueryBuilder, Transaction};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

macro_rules! dynamic_update {
    ($qb:ident, $sep:ident, $($field:expr => $sql_field:expr),* $(,)?) => {{
        $(
            if let Some(ref value) = $field {
                $sep.push(format!("{} = ", $sql_field)).push_bind(value);
            }
        )*
    }};
}

macro_rules! validate_config {
    ($config:expr, $($field:ident => $msg:expr),* $(,)?) => {
        $(
            if let Some(ref inner_config) = $config.$field {
                inner_config.validate()
                    .map_err(|e| VaultlessError::Validation(format!("{}: {}", $msg, e)))?;
            }
        )*
    };
}

impl Application {
    pub async fn update(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        update: UpdateApplication,
        application_id: Uuid,
        user_id: Uuid,
    ) -> Result<Application> {
        update
            .validate()
            .map_err(|e| VaultlessError::Validation(format!("Invalid update: {}", e)))?;

        let integrity_patch_opt: Option<JsonValue> = if let Some(ref cfg) = update.integrity_config
        {
            cfg.validate().map_err(|e| {
                VaultlessError::Validation(format!("Integrity config invalid: {}", e))
            })?;

            Some(
                serde_json::to_value(cfg)
                    .map_err(|e| VaultlessError::Serialization(e.to_string()))?,
            )
        } else {
            None
        };

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
        {
            let mut separated = qb.separated(", ");

            dynamic_update!(
                qb,
                separated,
                update.name => "name",
                update.description => "description",
                update.is_active => "is_active",
                update.max_ttl_seconds => "max_ttl_seconds",
                update.is_key_rotation_forced => "is_key_rotation_forced",
                update.internal_notes => "internal_notes",
            );

            if let Some(patch) = &integrity_patch_opt {
                separated
                    .push("integrity_config = jsonb_merge_patch(integrity_config, ")
                    .push_bind(patch)
                    .push(")");
            }
        }

        let built_sql = qb.sql();
        if built_sql.trim_end().ends_with("SET")
            || built_sql
                .trim()
                .eq_ignore_ascii_case("UPDATE applications SET")
        {
            tracing::info!(application_id = %application_id, "No fields to update");
            return Self::find_by_id_and_user_id(&*exec, application_id, user_id).await;
        }

        qb.push(" , updated_at = NOW() WHERE id = ")
            .push_bind(application_id)
            .push(" AND user_id = ")
            .push_bind(user_id)
            .push(" RETURNING *");

        let updated_app = qb.build_query_as::<Application>().fetch_one(&*exec).await?;

        if integrity_patch_opt.is_some() {
            Self::validate_integrity_config(&updated_app.integrity_config)?;
        }

        if let Some(pool) = redis {
            let exec_clone = exec.clone();
            tokio::spawn(async move {
                Self::invalidate_caches(exec_clone, pool, application_id).await;
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
        update
            .validate()
            .map_err(|e| VaultlessError::Validation(format!("Invalid update: {}", e)))?;

        let integrity_patch_opt: Option<JsonValue> = if let Some(ref cfg) = update.integrity_config
        {
            cfg.validate().map_err(|e| {
                VaultlessError::Validation(format!("Integrity config invalid: {}", e))
            })?;
            Some(
                serde_json::to_value(cfg)
                    .map_err(|e| VaultlessError::Serialization(e.to_string()))?,
            )
        } else {
            None
        };

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
        {
            let mut separated = qb.separated(", ");

            dynamic_update!(
                qb,
                separated,
                update.name => "name",
                update.description => "description",
                update.is_active => "is_active",
                update.max_ttl_seconds => "max_ttl_seconds",
                update.is_key_rotation_forced => "is_key_rotation_forced",
                update.internal_notes => "internal_notes",
            );

            if let Some(patch) = &integrity_patch_opt {
                separated
                    .push("integrity_config = jsonb_merge_patch(integrity_config, ")
                    .push_bind(patch)
                    .push(")");
            }
        }

        let built_sql = qb.sql();
        if built_sql.trim_end().ends_with("SET")
            || built_sql
                .trim()
                .eq_ignore_ascii_case("UPDATE applications SET")
        {
            let existing: Application = sqlx::query_as::<_, Application>(
                "SELECT * FROM applications WHERE id = $1 AND user_id = $2",
            )
            .bind(application_id)
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await?;
            return Ok(existing);
        }

        qb.push(" , updated_at = NOW() WHERE id = ")
            .push_bind(application_id)
            .push(" AND user_id = ")
            .push_bind(user_id)
            .push(" RETURNING *");

        let updated_app = qb
            .build_query_as::<Application>()
            .fetch_one(&mut **tx)
            .await?;

        if integrity_patch_opt.is_some() {
            Self::validate_integrity_config(&updated_app.integrity_config)?;
        }

        Ok(updated_app)
    }

    fn validate_integrity_config(config_json: &serde_json::Value) -> Result<()> {
        let config: IntegrityConfig = serde_json::from_value(config_json.clone())
            .map_err(|e| VaultlessError::Validation(format!("Invalid config: {}", e)))?;

        validate_config!(
            config,
            browser => "Browser config invalid",
            ios => "iOS config invalid",
            android => "Android config invalid",
            iot => "IoT config invalid",
            rate_limits => "Rate limits invalid",
        );

        Ok(())
    }

    async fn invalidate_caches(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Arc<RedisPool>,
        application_id: Uuid,
    ) {
        super::material_view_helper::trigger_view_refresh_debounced(exec.clone(), redis.clone());

        tokio::spawn(async move {
            if let Err(e) = Self::invalidate_auth_cache(application_id, &exec, redis).await {
                tracing::error!(
                    application_id = %application_id,
                    error = %e,
                    "Cache invalidation failed"
                );
            }
        });
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
