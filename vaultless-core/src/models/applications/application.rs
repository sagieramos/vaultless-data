use super::dto::*;
use super::integrity::integrity_handler::IntegrityConfigHandler;
use crate::cache_key;
use crate::crypto;
use crate::error::{Result, VaultlessError};
use crate::models::{ApiKey, CreateApiKey};
use crate::types::KeyType;
use deadpool_redis::Pool as RedisPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

const PROJECTION: &str = "id, user_id, subscription_id, name,
    description, is_active, created_at,
    updated_at, max_ttl_seconds, is_key_rotation_forced,
    deletion_requested_at,
    internal_notes, app_meta";

impl Application {
    /// Create a new application with secret and publishable keys
    pub async fn create(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        input: CreateApplication,
    ) -> Result<CreateApplicationResponse> {
        let mut tx = (*db_pool).begin().await.map_err(VaultlessError::Database)?;

        // Validate input
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // ============================================================
        // 1. CREATE APPLICATION FIRST
        // ============================================================
        let app = sqlx::query_as::<_, Application>(
            r#"
            INSERT INTO applications (
                user_id,
                name,
                description,
                max_ttl_seconds,
                is_key_rotation_forced
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(input.user_id)
        .bind(&input.name)
        .bind(&input.description)
        .bind(input.max_ttl_seconds.unwrap_or(604800))
        .bind(input.is_key_rotation_forced.unwrap_or(false))
        .fetch_one(&mut *tx)
        .await?;

        // ============================================================
        // 2. CREATE SECRET KEY
        // ============================================================
        let secret_key = crypto::generate_api_key("sk", "live")?;
        let secret_key_hash = crypto::hash_content(secret_key.as_bytes());
        let secret_key_prefix = secret_key.chars().take(8).collect::<String>();

        let _created_secret_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id: Some(input.user_id),
                key_hash: Some(secret_key_hash),
                key_prefix: secret_key_prefix,
                description: Some(format!("Secret key for {}", input.name)),
                scopes: None,
                expires_at: None,
                application_id: Some(app.id),
                key_type: crate::types::KeyType::Secret,
                publishable_key_plaintext: None,
            },
        )
        .await?;

        // ============================================================
        // 3. CREATE PUBLISHABLE KEY
        // ============================================================
        let publishable_key = crypto::generate_api_key("pk", "live")?;
        let pk_prefix = publishable_key.chars().take(16).collect::<String>();

        let _created_publishable_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id: Some(input.user_id),
                key_hash: None,
                key_prefix: pk_prefix,
                description: Some(format!("Publishable key for {}", input.name)),
                scopes: None,
                expires_at: None,
                application_id: Some(app.id),
                key_type: crate::types::KeyType::Publishable,
                publishable_key_plaintext: Some(publishable_key.clone()),
            },
        )
        .await?;

        // Commit
        tx.commit().await?;

        if let Some(redis_pool) = redis {
            super::material_view_helper::trigger_view_refresh(db_pool.clone(), redis_pool.clone());
        }

        tracing::info!(
            application_id = %app.id,
            "Application created with secret + publishable keys"
        );

        Ok(CreateApplicationResponse {
            application: app,
            secret_key: Some(secret_key),
            publishable_key_plaintext: publishable_key,
        })
    }

    /// Find application by ID
    pub async fn find_by_id<'c, E>(exec: E, id: Uuid) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Fetch from DB
        sqlx::query_as::<_, Application>(&format!(
            r#"
                SELECT {}
                FROM applications WHERE id = $1
                "#,
            PROJECTION
        ))
        .bind(id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found.".to_string()))
    }

    // Find application by ID and User ID
    pub async fn find_by_id_and_user_id<'c, E>(
        exec: E,
        id: Uuid,
        user_id: Uuid,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as::<_, Application>(&format!(
            r#"
                SELECT {}
                FROM applications WHERE id = $1 AND user_id = $2
                "#,
            PROJECTION
        ))
        .bind(id)
        .bind(user_id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found.".to_string()))
    }

    /// Find application by publishable key (for client registration) (UNCHANGED logic)
    pub async fn find_by_publishable_key<'c, E>(
        exec: E,
        publishable_key: &str,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Logic remains correct as it JOINs api_keys to find application_id
        // FIXED: Bind key_type as string (assuming enum stored as string)
        let key_type_str = crate::types::KeyType::Publishable.to_string();
        let app = sqlx::query_as::<_, Application>(&format!(
            r#"
            SELECT a.{} FROM applications a
            JOIN api_keys ak ON a.id = ak.application_id
            WHERE ak.publishable_key_plaintext = $1
              AND ak.key_type = $2
              AND a.is_active = true
            "#,
            PROJECTION // Use the updated projection here
        ))
        .bind(publishable_key)
        .bind(&key_type_str)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        Ok(app)
    }

    /// Helper to find the secret key ID associated with this application.
    /// This is needed because `secret_key_id` was removed from the Application struct.
    pub async fn find_secret_key_id<'c, E>(exec: E, app_id: Uuid) -> Result<Uuid>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_scalar(
            r#"
            SELECT id FROM api_keys 
            WHERE application_id = $1 AND key_type = $2
            "#,
        )
        .bind(app_id)
        .bind(KeyType::Secret.to_string())
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Associated Secret Key not found.".to_string()))
    }

    pub async fn deactivate_deep(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
    ) -> Result<()> {
        let row = sqlx::query(
        "UPDATE applications SET is_active = false, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        )
        .bind(app_id)
        .bind(user_id)
        .fetch_optional(exec.as_ref())
        .await?;

        let Some(_) = row else {
            return Err(VaultlessError::NotFound(format!(
                "Application not found or access denied for ID: {}",
                app_id
            )));
        };

        sqlx::query(
            "UPDATE api_keys SET is_active = false, updated_at = NOW() WHERE application_id = $1",
        )
        .bind(app_id)
        .execute(exec.as_ref())
        .await?;

        // 4. Handle cache invalidation
        if let Some(redis_pool) = redis {
            super::material_view_helper::trigger_view_refresh_debounced(
                exec.clone(),
                redis_pool.clone(),
            );
            tokio::spawn(async move {
                if let Err(e) = Self::invalidate_auth_cache(app_id, &exec, redis_pool).await {
                    tracing::error!(
                        "Background cache invalidation failed for app {}: {}",
                        app_id,
                        e
                    );
                }
            });
        } else {
            tracing::warn!(
                "Redis pool not provided. Skipping cache invalidation for deactivated app {}.",
                app_id
            );
        }
        Ok(())
    }

    pub async fn deactivate_weak(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
    ) -> Result<()> {
        let row = sqlx::query!(
            r#"
        UPDATE applications
        SET is_active = false, updated_at = NOW()
        WHERE id = $1 AND developer_id = $2
        RETURNING id
        "#,
            app_id,
            user_id
        )
        .fetch_optional(exec.as_ref())
        .await?;

        let Some(app_row) = row else {
            return Err(VaultlessError::NotFound(format!(
                "Application not found or access denied for ID: {}",
                app_id
            )));
        };

        if let Some(redis_pool) = redis {
            super::material_view_helper::trigger_view_refresh_debounced(
                exec.clone(),
                redis_pool.clone(),
            );
            tokio::spawn(async move {
                if let Err(e) = Self::invalidate_auth_cache(app_row.id, &exec, redis_pool).await {
                    tracing::error!(
                        "Background cache invalidation failed for app {}: {}",
                        app_row.id,
                        e
                    );
                }
            });
        }

        Ok(())
    }

    pub async fn delete<'c, E>(
        exec: Arc<sqlx::Pool<Postgres>>,
        app_id: Uuid,
        redis: Option<Arc<RedisPool>>,
        user_id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone + 'static,
    {
        let result = sqlx::query("DELETE FROM applications WHERE id = $1 AND user_id = $2")
            .bind(app_id)
            .bind(user_id)
            .execute(exec.as_ref())
            .await?;

        if result.rows_affected() == 0 {
            return Err(VaultlessError::NotFound(
                "Application not found or you don't have permission to delete it".into(),
            ));
        }

        // 3. Invalidate cache using spawned worker
        if let Some(redis_pool) = redis {
            super::material_view_helper::trigger_view_refresh_debounced(
                exec.clone(),
                redis_pool.clone(),
            );
            tokio::spawn(async move {
                if let Err(e) = Self::invalidate_auth_cache(app_id, &exec, redis_pool).await {
                    tracing::error!(
                        "Background cache invalidation failed for app {}: {}",
                        app_id,
                        e
                    );
                }
            });
        }

        Ok(())
    }

    pub async fn get_live_usage(
        redis_pool: Arc<RedisPool>,
        application_id: Uuid,
        quota_limit: i64,
    ) -> Result<QuotaStatus> {
        let quota_key = Application::quota_key(application_id);

        // 1. ACQUIRE CONNECTION from the pool
        // This is now the responsibility of the utility function.
        let mut conn = redis_pool.get().await.map_err(|e| {
            VaultlessError::Internal(format!("Failed to acquire Redis connection: {}", e))
        })?;

        // 2. Execute command using the acquired connection reference
        let monthly_count: Option<i64> = redis::cmd("GET")
            .arg(&quota_key)
            .query_async(&mut conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // When 'conn' goes out of scope here, it is automatically returned to the pool.

        let used = monthly_count.unwrap_or(0);
        let remaining = quota_limit.saturating_sub(used);
        let percentage_used = if quota_limit > 0 {
            (used as f64 / quota_limit as f64 * 100.0).min(100.0)
        } else {
            0.0
        };

        Ok(QuotaStatus {
            limit: quota_limit,
            used,
            remaining,
            percentage_used,
            is_exceeded: used >= quota_limit,
        })
    }

    pub fn integrity(&self) -> Result<IntegrityConfigHandler> {
        IntegrityConfigHandler::new_from_jsonb(&serde_json::to_value(&self.app_meta.0)?)
    }

    pub fn quota_key(application_id: Uuid) -> String {
        cache_key!("app", "quota", application_id)
    }
}
