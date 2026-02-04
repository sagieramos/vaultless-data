//! Key rotation methods for Application.
//!
//! Provides secure rotation of secret keys and publishable keys with:
//! - Atomic transaction-based rotation (old key deactivated, new key created)
//! - Cache invalidation for old keys
//! - Audit trail via old_key_id in response
//! - Optional grace period support for publishable keys

use super::dto::*;
use super::material_view_helper;
use crate::crypto;
use crate::error::{Result, VaultlessError};
use crate::types::KeyType;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;

/// Result returned from the SQL rotate_api_key function
#[derive(Debug, FromRow)]
struct RotateApiKeyResult {
    old_key_id: Uuid,
    new_key_id: Uuid,
    #[allow(dead_code)]
    key_type: KeyType,
    #[allow(dead_code)]
    scopes: Option<String>,
    old_key_hash: Option<String>,
    old_publishable_key_plaintext: Option<String>,
    created_at: DateTime<Utc>,
}

/// Result returned from the SQL add_publishable_key function
#[derive(Debug, FromRow)]
struct AddPublishableKeyResult {
    new_key_id: Uuid,
    key_prefix: String,
    created_at: DateTime<Utc>,
    total_active_publishable_keys: i64,
}

/// Result returned from the SQL deactivate_publishable_key function
#[derive(Debug, FromRow)]
struct DeactivatePublishableKeyResult {
    deactivated_key_id: Uuid,
    remaining_active_keys: i64,
}

impl Application {
    /// Rotate an API key (secret or publishable) for an application.
    ///
    /// This atomically:
    /// 1. Deactivates the current/specified key
    /// 2. Creates a new key with the same settings
    /// 3. Invalidates cached auth entries
    ///
    /// The new key is returned only once - store it securely!
    ///
    /// # Arguments
    /// * `db_pool` - Database connection pool
    /// * `redis` - Optional Redis pool for cache invalidation
    /// * `app_id` - The application ID
    /// * `user_id` - The user ID (for authorization)
    /// * `key_type` - Type of key to rotate (Secret or Publishable)
    /// * `old_publishable_key` - For publishable keys only: specific key to rotate (oldest if None)
    ///
    /// # Returns
    /// * Tuple of (new_key_plaintext, old_key_id, new_key_id, created_at)
    pub async fn rotate_key_internal(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
        key_type: KeyType,
        old_publishable_key: Option<&str>,
    ) -> Result<(String, String, Uuid, Uuid, DateTime<Utc>)> {
        // 1. Generate new key material based on type
        let (prefix, env, prefix_len, description) = match key_type {
            KeyType::Secret => ("sk", "live", 8, "Secret key (rotated)"),
            KeyType::Publishable => ("pk", "live", 16, "Publishable key (rotated)"),
        };

        let new_key_plaintext = crypto::generate_api_key(prefix, env)?;
        let new_key_prefix = new_key_plaintext
            .chars()
            .take(prefix_len)
            .collect::<String>();

        let (new_key_hash, new_publishable_key_plaintext) = match key_type {
            KeyType::Secret => (
                Some(crypto::hash_content(new_key_plaintext.as_bytes())),
                None,
            ),
            KeyType::Publishable => (None, Some(new_key_plaintext.as_str())),
        };

        // 2. Call SQL function to perform atomic rotation
        let result = sqlx::query_as::<_, RotateApiKeyResult>(
            r#"
            SELECT * FROM rotate_api_key(
                $1::UUID,      -- p_app_id
                $2::UUID,      -- p_user_id
                $3::key_type,  -- p_key_type
                $4::TEXT,      -- p_new_key_hash
                $5::TEXT,      -- p_new_key_prefix
                $6::TEXT,      -- p_new_publishable_key_plaintext
                $7::TEXT,      -- p_old_publishable_key
                $8::TEXT       -- p_description
            )
            "#,
        )
        .bind(app_id)
        .bind(user_id)
        .bind(key_type)
        .bind(new_key_hash.as_ref())
        .bind(&new_key_prefix)
        .bind(new_publishable_key_plaintext)
        .bind(old_publishable_key)
        .bind(description)
        .fetch_one(&*db_pool)
        .await
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Application not found") {
                VaultlessError::NotFound("Application not found or access denied".to_string())
            } else if error_msg.contains("Active") && error_msg.contains("key not found") {
                let msg = match key_type {
                    KeyType::Secret => "No active secret key found for application",
                    KeyType::Publishable => {
                        if old_publishable_key.is_some() {
                            "Specified publishable key not found or already inactive"
                        } else {
                            "No active publishable key found for application"
                        }
                    }
                };
                VaultlessError::NotFound(msg.to_string())
            } else if error_msg.contains("inactive") {
                VaultlessError::InvalidInput(
                    "Cannot rotate keys for inactive application".to_string(),
                )
            } else {
                VaultlessError::from(e)
            }
        })?;

        // 3. Invalidate old key cache (background task)
        if let Some(redis_pool) = redis.clone() {
            match key_type {
                KeyType::Secret => {
                    let old_key_hash = result.old_key_hash.clone();
                    tokio::spawn(async move {
                        if let Some(hash) = old_key_hash {
                            if let Ok(mut conn) = redis_pool.get().await {
                                let cache_key = secret_key_resolution_cache_key(hash.as_str());
                                let _: std::result::Result<(), _> = conn.del(&cache_key).await;
                            }
                        }
                    });
                }
                KeyType::Publishable => {
                    let old_pk_plaintext = result.old_publishable_key_plaintext.clone();
                    tokio::spawn(async move {
                        if let Some(pk) = old_pk_plaintext {
                            if let Ok(mut conn) = redis_pool.get().await {
                                let cache_key = publishable_key_resolution_cache_key(pk.as_str());
                                let _: std::result::Result<(), _> = conn.del(&cache_key).await;
                            }
                        }
                    });
                }
            }
        }

        // 4. Trigger materialized view refresh
        if let Some(redis_pool) = redis {
            material_view_helper::trigger_view_refresh_debounced(db_pool, redis_pool);
        }

        tracing::info!(
            application_id = %app_id,
            old_key_id = %result.old_key_id,
            new_key_id = %result.new_key_id,
            key_type = ?key_type,
            "API key rotated successfully"
        );

        Ok((
            new_key_plaintext,
            new_key_prefix,
            result.old_key_id,
            result.new_key_id,
            result.created_at,
        ))
    }

    /// Rotate the secret key for an application.
    ///
    /// This atomically:
    /// 1. Deactivates the current secret key
    /// 2. Creates a new secret key with the same tier/quota settings
    /// 3. Invalidates cached auth entries
    ///
    /// The new secret key is returned only once - store it securely!
    ///
    /// # Arguments
    /// * `db_pool` - Database connection pool
    /// * `redis` - Optional Redis pool for cache invalidation
    /// * `app_id` - The application ID
    /// * `user_id` - The user ID (for authorization)
    ///
    /// # Returns
    /// * `RotateSecretKeyResponse` containing the new secret key
    pub async fn rotate_secret_key(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
    ) -> Result<RotateSecretKeyResponse> {
        let (new_secret_key, key_prefix, old_key_id, _new_key_id, created_at) =
            Self::rotate_key_internal(db_pool, redis, app_id, user_id, KeyType::Secret, None)
                .await?;

        Ok(RotateSecretKeyResponse {
            application_id: app_id,
            new_secret_key,
            key_prefix,
            created_at,
            old_key_id,
        })
    }

    /// Rotate a publishable key for an application.
    ///
    /// This atomically:
    /// 1. Deactivates the specified publishable key (or the oldest active one if not specified)
    /// 2. Creates a new publishable key
    /// 3. Invalidates cached auth entries
    ///
    /// # Arguments
    /// * `db_pool` - Database connection pool
    /// * `redis` - Optional Redis pool for cache invalidation
    /// * `app_id` - The application ID
    /// * `user_id` - The user ID (for authorization)
    /// * `old_publishable_key` - Optional specific publishable key to rotate (deactivates oldest if None)
    ///
    /// # Returns
    /// * `RotatePublishableKeyResponse` containing the new publishable key
    pub async fn rotate_publishable_key(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
        old_publishable_key: Option<&str>,
    ) -> Result<RotatePublishableKeyResponse> {
        let (new_publishable_key, key_prefix, old_key_id, _new_key_id, created_at) =
            Self::rotate_key_internal(
                db_pool,
                redis,
                app_id,
                user_id,
                KeyType::Publishable,
                old_publishable_key,
            )
            .await?;

        Ok(RotatePublishableKeyResponse {
            application_id: app_id,
            new_publishable_key,
            key_prefix,
            created_at,
            old_key_id,
        })
    }

    /// Add an additional publishable key to an application.
    ///
    /// This allows multiple publishable keys to exist simultaneously,
    /// useful for gradual key rotation or multi-environment deployments.
    ///
    /// # Arguments
    /// * `db_pool` - Database connection pool
    /// * `redis` - Optional Redis pool for cache management
    /// * `app_id` - The application ID
    /// * `user_id` - The user ID (for authorization)
    /// * `environment` - Key environment (e.g., "live", "test", max 4 chars)
    /// * `max_keys` - Maximum allowed publishable keys (default: 5)
    ///
    /// # Returns
    /// * `AddPublishableKeyResponse` containing the new publishable key
    pub async fn add_publishable_key(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
        environment: Option<&str>,
        max_keys: Option<i64>,
    ) -> Result<AddPublishableKeyResponse> {
        let max_keys = max_keys.unwrap_or(5);
        let environment = environment.unwrap_or("live");

        // Validate environment length (max 4 characters)
        if environment.len() > 4 || environment.len() < 4 {
            return Err(VaultlessError::InvalidInput(
                "Environment must be 4 characters or less".to_string(),
            ));
        }

        // Generate new publishable key in application layer for security
        let new_publishable_key = crypto::generate_api_key("pk", environment)?;
        let new_key_prefix = new_publishable_key.chars().take(16).collect::<String>();

        // Call PostgreSQL function to add the key atomically
        let result = sqlx::query_as::<_, AddPublishableKeyResult>(
            r#"
            SELECT * FROM add_publishable_key($1, $2, $3, $4, $5)
            "#,
        )
        .bind(app_id)
        .bind(user_id)
        .bind(&new_publishable_key)
        .bind(&new_key_prefix)
        .bind(max_keys as i32)
        .fetch_one(db_pool.as_ref())
        .await?;

        // Trigger materialized view refresh
        if let Some(redis_pool) = redis {
            material_view_helper::trigger_view_refresh_debounced(db_pool, redis_pool);
        }

        tracing::info!(
            application_id = %app_id,
            new_key_id = %result.new_key_id,
            total_active_publishable_keys = result.total_active_publishable_keys,
            "Additional publishable key created"
        );

        Ok(AddPublishableKeyResponse {
            application_id: app_id,
            new_publishable_key,
            key_prefix: result.key_prefix,
            created_at: result.created_at,
            total_active_publishable_keys: result.total_active_publishable_keys,
        })
    }

    /// Deactivate a specific publishable key by its plaintext value.
    ///
    /// This allows selectively removing a publishable key without creating a new one.
    /// Useful when cleaning up old keys after rotation.
    ///
    /// # Arguments
    /// * `db_pool` - Database connection pool
    /// * `redis` - Optional Redis pool for cache invalidation
    /// * `app_id` - The application ID
    /// * `user_id` - The user ID (for authorization)
    /// * `publishable_key` - The publishable key plaintext to deactivate
    ///
    /// # Returns
    /// * `Ok(())` on success
    pub async fn deactivate_publishable_key(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
        publishable_key: &str,
    ) -> Result<()> {
        // Call PostgreSQL function to deactivate the key atomically
        let result = sqlx::query_as::<_, DeactivatePublishableKeyResult>(
            r#"
            SELECT * FROM deactivate_publishable_key($1, $2, $3)
            "#,
        )
        .bind(app_id)
        .bind(user_id)
        .bind(publishable_key)
        .fetch_one(db_pool.as_ref())
        .await?;

        // Invalidate cache (background task)
        if let Some(redis_pool) = redis {
            material_view_helper::trigger_view_refresh_debounced(db_pool, redis_pool.clone());

            let pk_plaintext = publishable_key.to_string();
            tokio::spawn(async move {
                if let Ok(mut conn) = redis_pool.get().await {
                    let cache_key = publishable_key_resolution_cache_key(&pk_plaintext);
                    let _: std::result::Result<(), _> = conn.del(&cache_key).await;
                }
            });
        }

        tracing::info!(
            application_id = %app_id,
            deactivated_key_id = %result.deactivated_key_id,
            remaining_active_keys = result.remaining_active_keys,
            "Publishable key deactivated"
        );

        Ok(())
    }
}
