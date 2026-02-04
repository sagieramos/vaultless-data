use super::dto::*;
use super::integrity::integrity_handler::IntegrityConfigHandler;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::types::KeyType;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use hex;
use sha2::{Digest, Sha256};
use sqlx::QueryBuilder;
use sqlx::{Executor, FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

pub(crate) const PROJECTION: &str = "id, developer_id AS user_id, subscription_id, name,
    description, is_active, created_at,
    updated_at, max_ttl_seconds, is_key_rotation_forced,
    deletion_requested_at,
    internal_notes, app_meta";

// Struct to match the PostgreSQL function return type
#[derive(Debug, FromRow)]
struct PgCreateApplicationResult {
    application_id: Uuid,
    user_id: Uuid,
    subscription_id: Option<Uuid>,
    name: String,
    description: Option<String>,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    max_ttl_seconds: Option<i64>,
    is_key_rotation_forced: bool,
    deletion_requested_at: Option<DateTime<Utc>>,
    internal_notes: Option<String>,
    app_meta: serde_json::Value,
    _secret_key_prefix: String,
    _publishable_key_plaintext: String,
}

impl Application {
    /// Create a new application with secret and publishable keys
    ///
    /// This function:
    /// 1. Generates secret and publishable keys in the application layer (secure)
    /// 2. Creates the application and keys atomically in the database
    /// 3. Returns the plaintext keys (only shown once - store securely!)
    ///
    /// # Arguments
    /// * `db_pool` - Database connection pool
    /// * `redis` - Optional Redis pool for cache management
    /// * `input` - Application creation parameters
    ///
    /// # Returns
    /// * `CreateApplicationResponse` containing the application and both keys
    pub async fn create(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        input: CreateApplication,
    ) -> Result<CreateApplicationResponse> {
        // Validate input
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Generate keys in the application layer for security
        let environment = input.environment.as_deref().unwrap_or("live");

        // Generate secret key
        let secret_key = crate::crypto::generate_api_key("sk", environment)?;
        let secret_key_hash = crate::crypto::hash_content(secret_key.as_bytes());
        let secret_key_prefix = secret_key.chars().take(8).collect::<String>();

        // Generate publishable key
        let publishable_key = crate::crypto::generate_api_key("pk", environment)?;
        let publishable_key_prefix = publishable_key.chars().take(16).collect::<String>();

        // Call the PostgreSQL function to create the application and keys
        let result = sqlx::query_as::<_, PgCreateApplicationResult>(
            r#"
            SELECT * FROM create_application(
                $1,  -- p_user_id
                $2,  -- p_name
                $3,  -- p_description
                $4,  -- p_max_ttl_seconds
                $5,  -- p_is_key_rotation_forced
                $6,  -- p_secret_key_hash
                $7,  -- p_secret_key_prefix
                $8,  -- p_publishable_key_plaintext
                $9   -- p_publishable_key_prefix
            )
            "#,
        )
        .bind(input.user_id)
        .bind(&input.name)
        .bind(&input.description)
        .bind(input.max_ttl_seconds)
        .bind(input.is_key_rotation_forced)
        .bind(&secret_key_hash)
        .bind(&secret_key_prefix)
        .bind(&publishable_key)
        .bind(&publishable_key_prefix)
        .fetch_one(db_pool.as_ref())
        .await?;

        if let Some(redis_pool) = redis {
            super::material_view_helper::trigger_view_refresh(db_pool.clone(), redis_pool.clone());
        }

        tracing::info!(
            application_id = %result.application_id,
            secret_key_prefix = %secret_key_prefix,
            publishable_key_prefix = %publishable_key_prefix,
            "Application created with secret + publishable keys"
        );

        Ok(CreateApplicationResponse {
            application: Application {
                id: result.application_id,
                user_id: result.user_id,
                subscription_id: result.subscription_id,
                name: result.name,
                description: result.description,
                is_active: result.is_active,
                created_at: result.created_at,
                updated_at: result.updated_at,
                max_ttl_seconds: result.max_ttl_seconds.unwrap_or(604800) as i32,
                is_key_rotation_forced: result.is_key_rotation_forced,
                deletion_requested_at: result.deletion_requested_at,
                internal_notes: result.internal_notes,
                app_meta: sqlx::types::Json(
                    serde_json::from_value(result.app_meta)
                        .unwrap_or_else(|_| super::integrity::dto::AppMetaData::default())
                ),
            },
            secret_key: Some(secret_key),
            publishable_key_plaintext: publishable_key,
        })
    }

    /// Find an application based on various filters with optional joins for API keys 
    pub async fn find<'c, E>(exec: E, filter: ApplicationFilter<'_>) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let mut qb = QueryBuilder::new(format!("SELECT {} FROM applications a ", PROJECTION));

        if filter.publishable_key.is_some() || filter.secret_key.is_some() {
            qb.push("JOIN api_keys ak ON a.id = ak.application_id ");
        }

        qb.push("WHERE 1=1 ");

        if let Some(id) = filter.id {
            qb.push(" AND a.id = ");
            qb.push_bind(id);
        }

        if let Some(dev_id) = filter.developer_id {
            qb.push(" AND a.developer_id = ");
            qb.push_bind(dev_id);
        }

        if let Some(is_active) = filter.is_active {
            qb.push(" AND a.is_active = ");
            qb.push_bind(is_active);
        }

        if let Some(pk) = filter.publishable_key {
            qb.push(" AND ak.publishable_key_plaintext = ");
            qb.push_bind(pk);

            qb.push(" AND ak.key_type = ");
            qb.push_bind(KeyType::Publishable.to_string());
        }

        if let Some(sk) = filter.secret_key {
            let mut hasher = Sha256::new();
            hasher.update(sk.as_bytes());
            let key_hash = hex::encode(hasher.finalize());

            qb.push(" AND ak.key_hash = ");
            qb.push_bind(key_hash);

            qb.push(" AND ak.key_type = ");
            qb.push_bind(KeyType::Secret.to_string());
        }

        let query = qb.build_query_as::<Application>();

        query
            .fetch_optional(exec)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Application not found.".to_string()))
    }

    pub async fn deactivate_deep(
        exec: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
    ) -> Result<()> {
        let row = sqlx::query(
        "UPDATE applications SET is_active = false, updated_at = NOW() WHERE id = $1 AND developer_id = $2",
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

    pub async fn delete<'c, E>(
        exec: Arc<sqlx::Pool<Postgres>>,
        app_id: Uuid,
        redis: Option<Arc<RedisPool>>,
        user_id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone + 'static,
    {
        let result = sqlx::query("DELETE FROM applications WHERE id = $1 AND developer_id = $2")
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

    pub fn integrity(&self) -> Result<IntegrityConfigHandler> {
        IntegrityConfigHandler::new_from_jsonb(&serde_json::to_value(&self.app_meta.0)?)
    }

    pub fn quota_key(application_id: Uuid) -> String {
        cache_key!("app", "quota", application_id)
    }
}
pub struct ApplicationFilter<'a> {
    pub id: Option<Uuid>,
    pub developer_id: Option<Uuid>,
    pub publishable_key: Option<&'a str>,
    pub secret_key: Option<&'a str>,
    pub is_active: Option<bool>,
}

impl<'a> ApplicationFilter<'a> {
    pub fn new() -> Self {
        Self {
            id: None,
            developer_id: None,
            publishable_key: None,
            secret_key: None,
            is_active: None,
        }
    }

    pub fn id(mut self, id: Uuid) -> Self {
        self.id = Some(id);
        self
    }

    pub fn developer_id(mut self, developer_id: Uuid) -> Self {
        self.developer_id = Some(developer_id);
        self
    }

    pub fn publishable_key(mut self, publishable_key: &'a str) -> Self {
        self.publishable_key = Some(publishable_key);
        self
    }

    pub fn secret_key(mut self, secret_key: &'a str) -> Self {
        self.secret_key = Some(secret_key);
        self
    }

    pub fn is_active(mut self, is_active: bool) -> Self {
        self.is_active = Some(is_active);
        self
    }
}
