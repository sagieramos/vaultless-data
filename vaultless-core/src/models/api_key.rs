//! # API Key Model
//!
//! Database-only model for API Key management.
//! Includes tier-based defaults and message quota checks directly against the database.

use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::types::{KeyType, SubscriptionTier}; // Import KeyType
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{self, info};
use uuid::Uuid;
use validator::Validate;

use crate::models::usage::{
    MetricCounters, MetricGranularity, MetricKey, REDIS_OPERATION_TIMEOUT_SECS,
};

// =============================================================================
// Constants
// =============================================================================

// Added key_type, application_id, publishable_key_plaintext
const PROJECTION: &str = "id, user_id, key_prefix, key_hash, tier, monthly_message_quota, message_retention_seconds, description, scopes, is_active, created_at, expires_at, last_used_at, rate_limit_per_minute, application_id, key_type, publishable_key_plaintext";

// =============================================================================
// Models
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub key_prefix: String,
    #[serde(skip_serializing)]
    pub key_hash: Option<String>,
    pub tier: Option<SubscriptionTier>,
    pub monthly_message_quota: i64,
    pub message_retention_seconds: i64,
    pub description: Option<String>,
    pub scopes: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub rate_limit_per_minute: i32,
    // New fields
    pub application_id: Option<Uuid>,
    pub key_type: KeyType,
    pub publishable_key_plaintext: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
    // key_prefix, key_hash, publishable_key_plaintext are omitted
    pub tier: Option<SubscriptionTier>,
    pub monthly_message_quota: i64,
    pub message_retention_seconds: i64,
    // description and scopes are omitted
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub rate_limit_per_minute: i32,
    pub application_id: Option<Uuid>,
    pub key_type: KeyType,
}

impl From<&ApiKey> for CachedApiKey {
    fn from(key: &ApiKey) -> Self {
        Self {
            id: key.id,
            user_id: key.user_id,
            // key_prefix, key_hash, and publishable_key_plaintext are omitted
            tier: key.tier,
            monthly_message_quota: key.monthly_message_quota,
            message_retention_seconds: key.message_retention_seconds,
            // description and scopes are omitted
            is_active: key.is_active,
            created_at: key.created_at,
            expires_at: key.expires_at,
            last_used_at: key.last_used_at,
            rate_limit_per_minute: key.rate_limit_per_minute,
            application_id: key.application_id,
            key_type: key.key_type,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedApiKeys {
    pub keys: Vec<ApiKey>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateApiKey {
    pub user_id: Uuid,

    // Made optional for Publishable keys
    pub key_hash: Option<String>,

    // Key prefix is mandatory for both
    pub key_prefix: String,

    // Made optional for Publishable keys
    pub tier: Option<SubscriptionTier>,

    #[validate(length(max = 255))]
    pub description: Option<String>,

    #[validate(length(min = 1))]
    pub scopes: Option<String>,

    pub expires_at: Option<DateTime<Utc>>,

    // New fields
    pub application_id: Option<Uuid>,
    pub key_type: KeyType,
    pub publishable_key_plaintext: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct LinkedKeyData {
    secret_key_hash: Option<String>,
    publishable_key_plaintext: Option<String>,
}

// =============================================================================
// Implementation
// =============================================================================

impl ApiKey {
    /// Creates a new API key with tier defaults (if Secret key).
    pub async fn create<'c, E>(executor: E, input: CreateApiKey) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Determine defaults based on key type
        let tier = input.tier.unwrap_or(SubscriptionTier::Free);

        let (monthly_quota, retention_seconds, rate_limit) = match input.key_type {
            KeyType::Secret => (
                tier.default_monthly_quota(),
                tier.default_retention_seconds(),
                tier.default_rate_limit(),
            ),
            // Publishable keys don't enforce these limits, use defaults/zeros
            // that satisfy the 'valid_quota' check constraint (which is conditional)
            KeyType::Publishable => (
                1000,   // Safe default that satisfies > 0 check if key_type changes
                604800, // Safe default that satisfies > 0 check if key_type changes
                60,
            ),
        };

        // Ensure Secret keys have hash/tier and Publishable keys have plaintext
        match input.key_type {
            KeyType::Secret => {
                if input.key_hash.is_none() || input.tier.is_none() {
                    return Err(VaultlessError::Validation(
                        "Secret key must provide hash and tier".to_string(),
                    ));
                }
                if input.publishable_key_plaintext.is_some() {
                    return Err(VaultlessError::Validation(
                        "Secret key cannot have plaintext data".to_string(),
                    ));
                }
            }
            KeyType::Publishable => {
                if input.publishable_key_plaintext.is_none() {
                    return Err(VaultlessError::Validation(
                        "Publishable key must provide plaintext data".to_string(),
                    ));
                }
                if input.key_hash.is_some() {
                    return Err(VaultlessError::Validation(
                        "Publishable key cannot have a hash".to_string(),
                    ));
                }
            }
        }

        let api_key = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                INSERT INTO api_keys (
                    user_id,
                    key_hash,
                    key_prefix,
                    tier,
                    monthly_message_quota,
                    message_retention_seconds,
                    description,
                    scopes,
                    rate_limit_per_minute,
                    expires_at,
                    is_active,
                    application_id, 
                    key_type,       
                    publishable_key_plaintext 
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, true, $11, $12, $13)
                RETURNING {}
                "#,
            PROJECTION
        ))
        .bind(input.user_id)
        .bind(&input.key_hash) // Now Option<String>
        .bind(&input.key_prefix)
        .bind(input.tier) // Now Option<SubscriptionTier>
        .bind(monthly_quota)
        .bind(retention_seconds)
        .bind(&input.description)
        .bind(&input.scopes)
        .bind(rate_limit)
        .bind(input.expires_at)
        .bind(input.application_id)
        .bind(input.key_type)
        .bind(&input.publishable_key_plaintext)
        .fetch_one(executor)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("API key already exists".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        Ok(api_key)
    }

    /// Finds API key by hash (for Secret keys) (Database only).
    pub async fn find_by_hash<'c, E>(exec: E, key_hash: String) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Database Lookup, restricted to Secret key type
        sqlx::query_as::<_, ApiKey>(&format!(
            r#"
        SELECT {}
        FROM api_keys
        WHERE key_hash = $1 AND key_type = 'secret'
        LIMIT 1
        "#,
            PROJECTION
        ))
        .bind(&key_hash)
        .fetch_optional(exec.clone())
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".into()))
    }

    /// Finds API key by full key (hashed internally), restricted to Secret key type.
    pub async fn find_by_api_key<'c, E>(exec: E, api_key: String) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
    {
        let key_hash = crate::crypto::hash_content(api_key.as_bytes());
        Self::find_by_hash(exec, key_hash).await
    }

    /// Finds API key by plaintext publishable key, restricted to Publishable key type.
    pub async fn find_by_publishable_key<'c, E>(exec: E, pk_plaintext: &str) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Database Lookup, restricted to Publishable key type
        sqlx::query_as::<_, ApiKey>(&format!(
            r#"
        SELECT {}
        FROM api_keys
        WHERE publishable_key_plaintext = $1 AND key_type = 'publishable'
        LIMIT 1
        "#,
            PROJECTION
        ))
        .bind(pk_plaintext)
        .fetch_optional(exec.clone())
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Publishable key not found".into()))
    }

    /// Finds API key by ID (Database only).
    pub async fn find_by_id<'c, E>(exec: E, id: Uuid) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Fetch from DB
        sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                SELECT {}
                FROM api_keys WHERE id = $1
                "#,
            PROJECTION
        ))
        .bind(id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".to_string()))
    }

    /// Lists API keys by owner (paginated with total count).
    pub async fn find_by_owner<'c, E>(
        exec: E,
        user_id: Uuid,
        page: Option<i64>,
        page_size: Option<i64>,
    ) -> Result<PaginatedApiKeys>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(50).clamp(1, 100);
        let offset = (page - 1) * page_size;

        // Get total count
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(exec.clone())
            .await?;

        // Get keys
        let keys = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                SELECT {}
                FROM api_keys 
                WHERE user_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
            PROJECTION
        ))
        .bind(user_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(exec)
        .await?;

        let has_more = (offset + page_size) < total;

        Ok(PaginatedApiKeys {
            keys,
            total,
            page,
            page_size,
            has_more,
        })
    }

    /// Lists all API keys (paginated with total count).
    // ... (This function remains unchanged as it queries all keys)
    pub async fn list<'c, E>(
        exec: E,
        page: Option<i64>,
        page_size: Option<i64>,
    ) -> Result<PaginatedApiKeys>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(50).clamp(1, 100);
        let offset = (page - 1) * page_size;

        // Get total count
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys")
            .fetch_one(exec.clone())
            .await?;

        // Get keys
        let keys = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                SELECT {}
                FROM api_keys 
                ORDER BY created_at DESC 
                LIMIT $1 OFFSET $2
                "#,
            PROJECTION
        ))
        .bind(page_size)
        .bind(offset)
        .fetch_all(exec)
        .await?;

        let has_more = (offset + page_size) < total;

        Ok(PaginatedApiKeys {
            keys,
            total,
            page,
            page_size,
            has_more,
        })
    }

    /// Validates active/expiry and checks quota against the database.
    /// Only Secret keys require quota validation.
    pub async fn validate<'c, E>(&self, _exec: E, redis_pool: Arc<RedisPool>) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. Check if the key is explicitly deactivated
        if !self.is_active {
            return Err(VaultlessError::ApiKeyInactive);
        }

        // 2. Check for expiry date
        if let Some(expires_at) = self.expires_at
            && expires_at < Utc::now()
        {
            return Err(VaultlessError::ApiKeyExpired);
        }

        // 3. Perform Quota Check (ONLY for Secret Keys)
        if self.key_type == KeyType::Secret {
            let quota = self.monthly_message_quota as i64;
            let is_quota_allowed = Self::check_quota(redis_pool, self.id, quota).await?;

            if !is_quota_allowed {
                return Err(VaultlessError::QuotaExceeded(
                    "Monthly message quota exceeded.".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Updates tier.
    pub async fn update_tier<'c, E>(exec: E, id: Uuid, new_tier: SubscriptionTier) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Perform the UPDATE (Tier update is restricted to Secret keys implicitly by the DB check)
        sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                UPDATE api_keys 
                SET 
                    tier = $2,
                    monthly_message_quota = $3,
                    message_retention_seconds = $4,
                    rate_limit_per_minute = $5
                WHERE id = $1 AND key_type = 'secret'
                RETURNING {}
                "#,
            PROJECTION
        ))
        .bind(id)
        .bind(new_tier)
        .bind(new_tier.default_monthly_quota())
        .bind(new_tier.default_retention_seconds())
        .bind(new_tier.default_rate_limit())
        .fetch_one(exec)
        .await
        .map_err(VaultlessError::Database)
    }

    // ... (deactivate, reactivate, update_metadata, delete remain largely the same)

    /// Quota check (Database only).
    pub async fn check_quota(
        redis_pool: Arc<RedisPool>,
        api_key_id: Uuid,
        quota: i64,
    ) -> Result<bool> {
        let count = Self::get_monthly_usage(redis_pool, api_key_id).await?;
        Ok(count < quota)
    }

    /// Generates the dedicated Redis key for real-time monthly quota tracking.
    /// This key relies on its TTL for monthly expiry.
    pub fn quota_cache_key(api_key_id: Uuid) -> String {
        // e.g., "quota:monthly:a854...-e49c"
        // This is the key that is atomically INCR'd in usage_metrics.rs
        cache_key!("quota", "monthly", api_key_id)
    }

    /// Retrieves the current monthly usage count from Redis.
    /// This count is based on real-time atomic increments performed on message send,
    /// suitable for quota enforcement of ephemeral data.
    pub async fn get_monthly_usage(redis_pool: Arc<RedisPool>, api_key_id: Uuid) -> Result<i64> // Assuming Result<i64> is defined
    {
        info!(api_key_id = %api_key_id, "Querying real-time monthly usage count from Redis");

        let monthly_key = Self::quota_cache_key(api_key_id);

        let count: i64 = match redis_pool.get().await {
            Ok(mut conn) => {
                // GET the counter value. If the key doesn't exist (e.g., first message of the month),
                // Redis returns 0 (None in Rust context, handled by unwrap_or).
                conn.get::<_, Option<i64>>(&monthly_key)
                    .await
                    .map_err(|e| VaultlessError::Internal(e.to_string()))?
                    .unwrap_or(0)
            }
            Err(e) => {
                tracing::error!("Redis connection error during quota check: {}", e);
                // Fail the operation if Redis is unavailable, as usage cannot be reliably verified.
                return Err(VaultlessError::Internal(format!(
                    "Redis usage check failed: {}",
                    e
                )));
            }
        };

        Ok(count)
    }

    /// Fetches the necessary key data (SK hash and PK plaintext) linked to an
    /// application ID for the sole purpose of invalidating Redis cache keys.
    pub async fn get_linked_key_data_for_cache_invalidation<'c, E>(
        exec: E,
        app_id: Uuid,
    ) -> Result<(Option<String>, String)>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // This query finds the hash of the Secret Key and the plaintext of
        // the Publishable Key associated with the given application ID.
        // We assume only one Secret Key and one Publishable Key per application.
        let data = sqlx::query_as::<_, LinkedKeyData>(
            r#"
            SELECT 
                MAX(CASE WHEN key_type = 'secret' THEN key_hash END) AS secret_key_hash,
                MAX(CASE WHEN key_type = 'publishable' THEN publishable_key_plaintext END) AS publishable_key_plaintext
            FROM api_keys
            WHERE application_id = $1
            "#,
        )
        .bind(app_id)
        .fetch_one(exec)
        .await
        .map_err(VaultlessError::Database)?;

        // The Publishable Key plaintext should *always* exist if the application was created successfully.
        let pk_plaintext = data.publishable_key_plaintext.ok_or_else(|| {
            VaultlessError::Internal(format!(
                "Could not find Publishable Key for application ID: {}",
                app_id
            ))
        })?;

        // Return (SK Hash, PK Plaintext)
        Ok((data.secret_key_hash, pk_plaintext))
    }

    pub async fn get_current_period_counters(
        redis_pool: Arc<RedisPool>,
        api_key_id: Uuid,
    ) -> Result<MetricCounters> {
        info!(api_key_id = %api_key_id, "Querying real-time hourly metric counters from Redis.");

        // 1. Determine the key for the current hourly period
        let now = Utc::now();
        let period_key = match MetricKey::new(api_key_id, now, MetricGranularity::Hour) {
            Ok(key) => key,
            Err(e) => {
                tracing::warn!("Failed to generate metric key: {}", e);
                // Return default/zero counters on failure
                return Ok(MetricCounters::default());
            }
        };

        // 2. Define the fields to retrieve from the Redis Hash
        let fields = [
            "messages_sent",
            "messages_received",
            "proofs_verified",
            "total_bytes_sent",
            "total_bytes_received",
            "rate_limit_hits",
        ];

        let mut conn = match redis_pool.get().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::error!("Redis connection error during metrics lookup: {}", e);
                // Return default/zero counters if Redis connection fails (fail open/soft)
                return Ok(MetricCounters::default());
            }
        };

        // 3. Perform the HGETALL or HMGET operation within a timeout
        let result = tokio::time::timeout(
            Duration::from_secs(REDIS_OPERATION_TIMEOUT_SECS),
            // HMGET is more efficient for fetching specific fields
            conn.hmget::<_, _, HashMap<String, i64>>(period_key.as_str(), &fields),
        )
        .await;

        // 4. Process the result
        match result {
            Ok(Ok(values_map)) => {
                let mut counters = MetricCounters::default();
                // Note: MetricCounters has a merge_from_map method,
                // but we can manually map for clarity in this specific lookup.

                counters.messages_sent = *values_map.get("messages_sent").unwrap_or(&0);
                counters.messages_received = *values_map.get("messages_received").unwrap_or(&0);
                counters.proofs_verified = *values_map.get("proofs_verified").unwrap_or(&0);
                counters.total_bytes_sent = *values_map.get("total_bytes_sent").unwrap_or(&0);
                counters.total_bytes_received =
                    *values_map.get("total_bytes_received").unwrap_or(&0);
                counters.rate_limit_hits = *values_map.get("rate_limit_hits").unwrap_or(&0);

                info!(
                    api_key_id = %api_key_id,
                    period_key = period_key.as_str(),
                    "Retrieved real-time metric counters."
                );

                Ok(counters)
            }
            Ok(Err(e)) => {
                tracing::error!("Redis HMGET error for key {}: {}", period_key.as_str(), e);
                // Return default/zero counters on Redis command error
                Ok(MetricCounters::default())
            }
            Err(_) => {
                tracing::warn!(
                    "Redis operation timed out during metrics lookup for key {}",
                    period_key.as_str()
                );
                // Return default/zero counters on timeout
                Ok(MetricCounters::default())
            }
        }
    }

    pub async fn find_by_plaintext_or_hash<'c, E>(
        exec: E,
        key_hash_hex: &str, // The hash of the key, used for lookup
    ) -> Result<(Self, KeyType)>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // For security, we only query against the stored hash in the 'key_hash' column.
        let row = sqlx::query_as::<_, ApiKey>(
            r#"
            SELECT
                id, 
                application_id, 
                key_hash
            FROM 
                api_keys
            WHERE 
                key_hash = $1
            AND 
                key_type = 'secret' -- Assuming you have a key_type column
            "#,
        )
        .bind(key_hash_hex)
        .fetch_optional(exec)
        .await?;

        match row {
            Some(key_row) => Ok((key_row, KeyType::Secret)),
            None => Err(crate::error::VaultlessError::NotFound(
                "API Key not found or key type is not Secret.".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_defaults() {
        assert_eq!(SubscriptionTier::Free.default_monthly_quota(), 1_000);
        assert_eq!(SubscriptionTier::Pro.default_rate_limit(), 1_000);
        assert_eq!(SubscriptionTier::Starter.monthly_price_cents(), Some(2_900));
    }
}
