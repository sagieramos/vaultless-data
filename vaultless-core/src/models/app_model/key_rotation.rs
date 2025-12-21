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
use crate::models::{ApiKey, CreateApiKey};
use crate::types::KeyType;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::Postgres;
use std::sync::Arc;
use uuid::Uuid;

impl Application {
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
        let mut tx = db_pool.begin().await?;

        // 1. Verify application exists and belongs to user
        let app = sqlx::query_as::<_, Application>(
            r#"
            SELECT id, user_id, name, description, is_active, created_at,
                   updated_at, max_ttl_seconds, is_key_rotation_forced,
                   deletion_requested_at, internal_notes, app_meta
            FROM applications
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(app_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Application not found or access denied".to_string())
        })?;

        if !app.is_active {
            return Err(VaultlessError::InvalidInput(
                "Cannot rotate keys for inactive application".to_string(),
            ));
        }

        // 2. Find the current active secret key
        let old_key: ApiKey = sqlx::query_as(
            r#"
            SELECT id, user_id, key_prefix, key_hash, tier, monthly_message_quota,
                   message_retention_seconds, description, scopes, is_active,
                   created_at, expires_at, last_used_at, rate_limit_per_minute,
                   application_id, key_type, publishable_key_plaintext
            FROM api_keys
            WHERE application_id = $1
              AND key_type = 'secret'
              AND is_active = true
            LIMIT 1
            "#,
        )
        .bind(app_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("No active secret key found for application".to_string())
        })?;

        // 3. Deactivate the old secret key
        sqlx::query(
            r#"
            UPDATE api_keys
            SET is_active = false, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(old_key.id)
        .execute(&mut *tx)
        .await?;

        // 4. Generate new secret key with same settings
        let new_secret_key = crypto::generate_api_key("sk", "live")?;
        let new_key_hash = crypto::hash_content(new_secret_key.as_bytes());
        let new_key_prefix = new_secret_key.chars().take(8).collect::<String>();

        let created_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id,
                key_hash: Some(new_key_hash),
                key_prefix: new_key_prefix.clone(),
                tier: old_key.tier,
                description: Some(format!("Secret key for {} (rotated)", app.name)),
                scopes: old_key.scopes.clone(),
                expires_at: None,
                application_id: Some(app_id),
                key_type: KeyType::Secret,
                publishable_key_plaintext: None,
            },
        )
        .await?;

        // 5. Commit transaction
        tx.commit().await?;

        // 6. Invalidate old key cache (background task)
        if let Some(redis_pool) = redis.clone() {
            let old_key_hash = old_key.key_hash.clone();
            tokio::spawn(async move {
                if let Some(hash) = old_key_hash {
                    if let Ok(mut conn) = redis_pool.get().await {
                        let cache_key = secret_key_resolution_cache_key(&hash);
                        let _: std::result::Result<(), _> = conn.del(&cache_key).await;
                    }
                }
            });
        }

        // 7. Trigger materialized view refresh
        if let Some(redis_pool) = redis {
            material_view_helper::trigger_view_refresh_debounced(db_pool, redis_pool);
        }

        tracing::info!(
            application_id = %app_id,
            old_key_id = %old_key.id,
            new_key_id = %created_key.id,
            "Secret key rotated successfully"
        );

        Ok(RotateSecretKeyResponse {
            application_id: app_id,
            new_secret_key,
            key_prefix: new_key_prefix,
            created_at: created_key.created_at,
            old_key_id: old_key.id,
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
    /// * `old_key_id` - Optional specific key ID to rotate (deactivates oldest if None)
    ///
    /// # Returns
    /// * `RotatePublishableKeyResponse` containing the new publishable key
    pub async fn rotate_publishable_key(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
        old_key_id: Option<Uuid>,
    ) -> Result<RotatePublishableKeyResponse> {
        let mut tx = db_pool.begin().await?;

        // 1. Verify application exists and belongs to user
        let app = sqlx::query_as::<_, Application>(
            r#"
            SELECT id, user_id, name, description, is_active, created_at,
                   updated_at, max_ttl_seconds, is_key_rotation_forced,
                   deletion_requested_at, internal_notes, app_meta
            FROM applications
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(app_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Application not found or access denied".to_string())
        })?;

        if !app.is_active {
            return Err(VaultlessError::InvalidInput(
                "Cannot rotate keys for inactive application".to_string(),
            ));
        }

        // 2. Find the publishable key to deactivate
        let old_key: ApiKey = if let Some(key_id) = old_key_id {
            // Specific key requested
            sqlx::query_as(
                r#"
                SELECT id, user_id, key_prefix, key_hash, tier, monthly_message_quota,
                       message_retention_seconds, description, scopes, is_active,
                       created_at, expires_at, last_used_at, rate_limit_per_minute,
                       application_id, key_type, publishable_key_plaintext
                FROM api_keys
                WHERE id = $1
                  AND application_id = $2
                  AND key_type = 'publishable'
                  AND is_active = true
                "#,
            )
            .bind(key_id)
            .bind(app_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                VaultlessError::NotFound(
                    "Specified publishable key not found or already inactive".to_string(),
                )
            })?
        } else {
            // Find the oldest active publishable key
            sqlx::query_as(
                r#"
                SELECT id, user_id, key_prefix, key_hash, tier, monthly_message_quota,
                       message_retention_seconds, description, scopes, is_active,
                       created_at, expires_at, last_used_at, rate_limit_per_minute,
                       application_id, key_type, publishable_key_plaintext
                FROM api_keys
                WHERE application_id = $1
                  AND key_type = 'publishable'
                  AND is_active = true
                ORDER BY created_at ASC
                LIMIT 1
                "#,
            )
            .bind(app_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                VaultlessError::NotFound(
                    "No active publishable key found for application".to_string(),
                )
            })?
        };

        // 3. Deactivate the old publishable key
        sqlx::query(
            r#"
            UPDATE api_keys
            SET is_active = false, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(old_key.id)
        .execute(&mut *tx)
        .await?;

        // 4. Generate new publishable key
        let new_publishable_key = crypto::generate_api_key("pk", "live")?;
        let new_key_prefix = new_publishable_key.chars().take(16).collect::<String>();

        let created_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id,
                key_hash: None,
                key_prefix: new_key_prefix.clone(),
                tier: None,
                description: Some(format!("Publishable key for {} (rotated)", app.name)),
                scopes: None,
                expires_at: None,
                application_id: Some(app_id),
                key_type: KeyType::Publishable,
                publishable_key_plaintext: Some(new_publishable_key.clone()),
            },
        )
        .await?;

        // 5. Commit transaction
        tx.commit().await?;

        // 6. Invalidate old key cache (background task)
        if let Some(redis_pool) = redis.clone() {
            let old_pk_plaintext = old_key.publishable_key_plaintext.clone();
            tokio::spawn(async move {
                if let Some(pk) = old_pk_plaintext {
                    if let Ok(mut conn) = redis_pool.get().await {
                        let cache_key = publishable_key_resolution_cache_key(&pk);
                        let _: std::result::Result<(), _> = conn.del(&cache_key).await;
                    }
                }
            });
        }

        // 7. Trigger materialized view refresh
        if let Some(redis_pool) = redis {
            material_view_helper::trigger_view_refresh_debounced(db_pool, redis_pool);
        }

        tracing::info!(
            application_id = %app_id,
            old_key_id = %old_key.id,
            new_key_id = %created_key.id,
            "Publishable key rotated successfully"
        );

        Ok(RotatePublishableKeyResponse {
            application_id: app_id,
            new_publishable_key,
            key_prefix: new_key_prefix,
            created_at: created_key.created_at,
            old_key_id: old_key.id,
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
    /// * `max_keys` - Maximum allowed publishable keys (default: 5)
    ///
    /// # Returns
    /// * `AddPublishableKeyResponse` containing the new publishable key
    pub async fn add_publishable_key(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
        max_keys: Option<i64>,
    ) -> Result<AddPublishableKeyResponse> {
        let max_keys = max_keys.unwrap_or(5);
        let mut tx = db_pool.begin().await?;

        // 1. Verify application exists and belongs to user
        let app = sqlx::query_as::<_, Application>(
            r#"
            SELECT id, user_id, name, description, is_active, created_at,
                   updated_at, max_ttl_seconds, is_key_rotation_forced,
                   deletion_requested_at, internal_notes, app_meta
            FROM applications
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(app_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Application not found or access denied".to_string())
        })?;

        if !app.is_active {
            return Err(VaultlessError::InvalidInput(
                "Cannot add keys to inactive application".to_string(),
            ));
        }

        // 2. Check current publishable key count
        let current_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM api_keys
            WHERE application_id = $1
              AND key_type = 'publishable'
              AND is_active = true
            "#,
        )
        .bind(app_id)
        .fetch_one(&mut *tx)
        .await?;

        if current_count >= max_keys {
            return Err(VaultlessError::InvalidInput(format!(
                "Maximum of {} active publishable keys allowed per application",
                max_keys
            )));
        }

        // 3. Generate new publishable key
        let new_publishable_key = crypto::generate_api_key("pk", "live")?;
        let new_key_prefix = new_publishable_key.chars().take(16).collect::<String>();

        let created_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id,
                key_hash: None,
                key_prefix: new_key_prefix.clone(),
                tier: None,
                description: Some(format!("Publishable key for {} (additional)", app.name)),
                scopes: None,
                expires_at: None,
                application_id: Some(app_id),
                key_type: KeyType::Publishable,
                publishable_key_plaintext: Some(new_publishable_key.clone()),
            },
        )
        .await?;

        // 4. Commit transaction
        tx.commit().await?;

        // 5. Trigger materialized view refresh
        if let Some(redis_pool) = redis {
            material_view_helper::trigger_view_refresh_debounced(db_pool, redis_pool);
        }

        let total_active = current_count + 1;

        tracing::info!(
            application_id = %app_id,
            new_key_id = %created_key.id,
            total_active_publishable_keys = total_active,
            "Additional publishable key created"
        );

        Ok(AddPublishableKeyResponse {
            application_id: app_id,
            new_publishable_key,
            key_prefix: new_key_prefix,
            created_at: created_key.created_at,
            total_active_publishable_keys: total_active,
        })
    }

    /// Deactivate a specific publishable key.
    ///
    /// This allows selectively removing a publishable key without creating a new one.
    /// Useful when cleaning up old keys after rotation.
    ///
    /// # Arguments
    /// * `db_pool` - Database connection pool
    /// * `redis` - Optional Redis pool for cache invalidation
    /// * `app_id` - The application ID
    /// * `user_id` - The user ID (for authorization)
    /// * `key_id` - The specific publishable key ID to deactivate
    ///
    /// # Returns
    /// * `Ok(())` on success
    pub async fn deactivate_publishable_key(
        db_pool: Arc<sqlx::Pool<Postgres>>,
        redis: Option<Arc<RedisPool>>,
        app_id: Uuid,
        user_id: Uuid,
        key_id: Uuid,
    ) -> Result<()> {
        let mut tx = db_pool.begin().await?;

        // 1. Verify application exists and belongs to user
        let _app = sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id FROM applications WHERE id = $1 AND user_id = $2"#,
        )
        .bind(app_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Application not found or access denied".to_string())
        })?;

        // 2. Check how many active publishable keys exist
        let active_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM api_keys
            WHERE application_id = $1
              AND key_type = 'publishable'
              AND is_active = true
            "#,
        )
        .bind(app_id)
        .fetch_one(&mut *tx)
        .await?;

        if active_count <= 1 {
            return Err(VaultlessError::InvalidInput(
                "Cannot deactivate the last active publishable key. Use rotate instead."
                    .to_string(),
            ));
        }

        // 3. Fetch the key to deactivate (and get plaintext for cache invalidation)
        let key: ApiKey = sqlx::query_as(
            r#"
            SELECT id, user_id, key_prefix, key_hash, tier, monthly_message_quota,
                   message_retention_seconds, description, scopes, is_active,
                   created_at, expires_at, last_used_at, rate_limit_per_minute,
                   application_id, key_type, publishable_key_plaintext
            FROM api_keys
            WHERE id = $1
              AND application_id = $2
              AND key_type = 'publishable'
              AND is_active = true
            "#,
        )
        .bind(key_id)
        .bind(app_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound(
                "Specified publishable key not found or already inactive".to_string(),
            )
        })?;

        // 4. Deactivate the key
        sqlx::query(
            r#"
            UPDATE api_keys
            SET is_active = false, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(key_id)
        .execute(&mut *tx)
        .await?;

        // 5. Commit transaction
        tx.commit().await?;

        // 6. Invalidate cache (background task)
        if let Some(redis_pool) = redis.clone() {
            let pk_plaintext = key.publishable_key_plaintext.clone();
            tokio::spawn(async move {
                if let Some(pk) = pk_plaintext {
                    if let Ok(mut conn) = redis_pool.get().await {
                        let cache_key = publishable_key_resolution_cache_key(&pk);
                        let _: std::result::Result<(), _> = conn.del(&cache_key).await;
                    }
                }
            });
        }

        // 7. Trigger materialized view refresh
        if let Some(redis_pool) = redis {
            material_view_helper::trigger_view_refresh_debounced(db_pool, redis_pool);
        }

        tracing::info!(
            application_id = %app_id,
            deactivated_key_id = %key_id,
            "Publishable key deactivated"
        );

        Ok(())
    }
}
