use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

use crate::cache_key;
use crate::crypto;
use crate::error::{Result, VaultlessError};
use crate::models::{ApiKey, CreateApiKey};
use crate::types::SubscriptionTier;

const APPLICATION_CACHE_TTL: u64 = 600; // 10 minutes

// =============================================================================
// Models
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Application {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub secret_key_id: Uuid,
    pub publishable_key: String,
    pub publishable_key_prefix: String,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateApplication {
    pub user_id: Uuid,

    #[validate(length(min = 1, max = 255))]
    pub name: String,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    pub tier: SubscriptionTier,

    #[validate(length(max = 255))]
    pub bundle_id: Option<String>,

    #[validate(length(max = 50))]
    pub platform: Option<String>,

    #[validate(url)]
    pub webhook_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateApplicationResponse {
    pub application: Application,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>, // Only returned once at creation
    pub publishable_key: String,
}

// =============================================================================
// Cache Keys
// =============================================================================

pub fn cache_key_by_publishable_key(pk: &str) -> String {
    cache_key!("app", "pk", pk)
}

pub fn cache_key_by_id(id: Uuid) -> String {
    cache_key!("app", "id", id)
}

// =============================================================================
// Implementation
// =============================================================================

impl Application {
    /// Create a new application with secret and publishable keys
    pub async fn create<'c, E>(
        exec: E,
        input: CreateApplication,
    ) -> Result<CreateApplicationResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // 1. Generate secret key (sk_live_... or sk_test_...)
        let secret_key = crypto::generate_api_key("sk", "live")?;
        let secret_key_hash = crypto::hash_content(secret_key.as_bytes());
        let secret_key_prefix = secret_key.chars().take(8).collect::<String>();

        // 2. Create API key entry (secret key)
        let api_key = ApiKey::create(
            exec.clone(),
            CreateApiKey {
                user_id: input.user_id,
                key_hash: secret_key_hash,
                key_prefix: secret_key_prefix,
                tier: input.tier,
                description: Some(format!("Secret key for {}", input.name)),
                scopes: None,
                expires_at: None,
            },
        )
        .await?;

        // 3. Generate publishable key (pk_live_...)
        let publishable_key = crypto::generate_api_key("pk", "live")?;
        let publishable_key_prefix = publishable_key.chars().take(16).collect::<String>();

        // 4. Create application
        let app = sqlx::query_as::<_, Application>(
            r#"
            INSERT INTO applications (
                user_id, name, description,
                secret_key_id, publishable_key, publishable_key_prefix,
                bundle_id, platform, webhook_url
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
        )
        .bind(input.user_id)
        .bind(&input.name)
        .bind(&input.description)
        .bind(api_key.id)
        .bind(&publishable_key)
        .bind(&publishable_key_prefix)
        .bind(&input.bundle_id)
        .bind(&input.platform)
        .bind(&input.webhook_url)
        .fetch_one(exec)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Application with this name already exists".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        tracing::info!(
            application_id = %app.id,
            user_id = %input.user_id,
            name = %input.name,
            "Application created successfully"
        );

        Ok(CreateApplicationResponse {
            application: app,
            secret_key: Some(secret_key),
            publishable_key,
        })
    }

    /// Find application by publishable key (for client registration)
    pub async fn find_by_publishable_key<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        publishable_key: &str,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = cache_key_by_publishable_key(publishable_key);

        // Try Redis cache first
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
                    if let Ok(app) = serde_json::from_str::<Application>(&cached_json) {
                        tracing::debug!("Cache hit for publishable key");
                        return Ok(app);
                    }
                }
            }
        }

        // Database lookup
        let app = sqlx::query_as::<_, Application>(
            r#"
            SELECT * FROM applications 
            WHERE publishable_key = $1 AND is_active = true
            "#,
        )
        .bind(publishable_key)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        // Cache it
        if let Some(redis_pool) = redis {
            if let Ok(serialized) = serde_json::to_string(&app) {
                tokio::spawn(async move {
                    if let Ok(mut conn) = redis_pool.get().await {
                        let _ = conn
                            .set_ex::<_, _, ()>(&cache_key, serialized, APPLICATION_CACHE_TTL)
                            .await;
                    }
                });
            }
        }

        Ok(app)
    }

    /// Find application by ID
    pub async fn find_by_id<'c, E>(exec: E, id: Uuid) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Database lookup
        let app = sqlx::query_as::<_, Application>(r#"SELECT * FROM applications WHERE id = $1"#)
            .bind(id)
            .fetch_optional(exec)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;
        Ok(app)
    }

    /// List applications by user
    pub async fn find_by_user<'c, E>(exec: E, user_id: Uuid) -> Result<Vec<Application>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let apps = sqlx::query_as::<_, Application>(
            r#"
            SELECT * FROM applications 
            WHERE user_id = $1 
            ORDER BY created_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(exec)
        .await?;

        Ok(apps)
    }

    /// Deactivate application
    pub async fn deactivate<'c, E>(exec: E, redis: Option<Arc<RedisPool>>, id: Uuid) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        sqlx::query("UPDATE applications SET is_active = false WHERE id = $1")
            .bind(id)
            .execute(exec.clone())
            .await?;

        // Fetch for cache invalidation
        let app = Self::find_by_id(exec.clone(), id).await?;
        let api = ApiKey::find_by_id(exec, redis.clone(), app.secret_key_id).await?;
        ApiKey::invalidate_cache(redis, app.secret_key_id, api.key_hash).await;
        Ok(())
    }

    /// Get the associated secret API key (for validation/billing)
    pub async fn get_secret_key<'c, E>(
        &self,
        exec: E,
        redis: Option<Arc<RedisPool>>,
    ) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        ApiKey::find_by_id(exec, redis, self.secret_key_id).await
    }

    /// Validate the application's secret key (quota, expiry, active status)
    pub async fn validate_secret_key<'c, E>(
        &self,
        exec: E,
        redis: Option<Arc<RedisPool>>,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let api_key = self.get_secret_key(exec.clone(), redis.clone()).await?;
        api_key.validate(exec, redis).await
    }

    /// Update the tier of the application's secret key
    /// This affects quota, rate limits, and billing for all clients using this app
    pub async fn update_tier<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        application_id: Uuid,
        new_tier: SubscriptionTier,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. Get the application to find the secret_key_id
        let app = Self::find_by_id(exec.clone(), application_id).await?;

        // 2. Update the underlying secret API key's tier
        let updated_api_key =
            ApiKey::update_tier(exec.clone(), redis.clone(), app.secret_key_id, new_tier).await?;

        tracing::info!(
            application_id = %application_id,
            api_key_id = %app.secret_key_id,
            old_tier = ?updated_api_key.tier,
            new_tier = ?new_tier,
            "Application tier updated"
        );

        // 3. Invalidate application cache (since tier info might be cached elsewhere)
        Self::invalidate_cache(redis, &app).await;

        // 4. Return the application (tier is stored in api_key, not application)
        Ok(app)
    }

    /// Get the current tier of this application
    pub async fn get_tier<'c, E>(
        &self,
        exec: E,
        redis: Option<Arc<RedisPool>>,
    ) -> Result<SubscriptionTier>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let api_key = ApiKey::find_by_id(exec, redis, self.secret_key_id).await?;
        Ok(api_key.tier)
    }

    /// Get full details including tier information (with JOIN)
    pub async fn find_by_id_with_tier<'c, E>(exec: E, id: Uuid) -> Result<ApplicationWithTier>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let result = sqlx::query_as::<_, ApplicationWithTier>(
            r#"
            SELECT 
                a.*,
                ak.tier,
                ak.monthly_message_quota,
                ak.rate_limit_per_minute,
                ak.message_retention_seconds,
                ak.is_active as api_key_active
            FROM applications a
            JOIN api_keys ak ON a.secret_key_id = ak.id
            WHERE a.id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        Ok(result)
    }

    /// List applications by user with tier info
    pub async fn find_by_user_with_tier<'c, E>(
        exec: E,
        user_id: Uuid,
    ) -> Result<Vec<ApplicationWithTier>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let apps = sqlx::query_as::<_, ApplicationWithTier>(
            r#"
            SELECT 
                a.*,
                ak.tier,
                ak.monthly_message_quota,
                ak.rate_limit_per_minute,
                ak.message_retention_seconds,
                ak.is_active as api_key_active
            FROM applications a
            JOIN api_keys ak ON a.secret_key_id = ak.id
            WHERE a.user_id = $1 
            ORDER BY a.created_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(exec)
        .await?;

        Ok(apps)
    }

    /// Helper to invalidate application cache
    async fn invalidate_cache(redis: Option<Arc<RedisPool>>, app: &Application) {
        if let Some(redis_pool) = redis {
            let app_id = app.id;
            let publishable_key = app.publishable_key.clone();

            tokio::spawn(async move {
                if let Ok(mut conn) = redis_pool.get().await {
                    let id_key = cache_key_by_id(app_id);
                    let pk_key = cache_key_by_publishable_key(&publishable_key); 

                    let _ = conn.del::<_, ()>(&id_key).await;
                    let _ = conn.del::<_, ()>(&pk_key).await;

                    tracing::debug!("Invalidated application cache for {}", app_id);
                }
            });
        }
    }
}

/// Application with denormalized tier information from api_keys
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApplicationWithTier {
    // All Application fields
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub secret_key_id: Uuid,
    pub publishable_key: String,
    pub publishable_key_prefix: String,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Tier information from api_keys JOIN
    pub tier: SubscriptionTier,
    pub monthly_message_quota: i32,
    pub rate_limit_per_minute: i32,
    pub message_retention_seconds: i32,
    pub api_key_active: bool,
}

impl ApplicationWithTier {
    /// Check if both application and API key are active
    pub fn is_fully_active(&self) -> bool {
        self.is_active && self.api_key_active
    }
}
