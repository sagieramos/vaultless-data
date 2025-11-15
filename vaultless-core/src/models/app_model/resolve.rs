use super::dto::*;
use crate::error::Result;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

impl Application {
    pub async fn fetch_auth_config_by_publishable_key<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        pk_plaintext: &str,
    ) -> Result<Option<AuthConfig>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = publishable_key_resolution_cache_key(pk_plaintext);

        // --- HOT PATH ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(Some(cached)) = conn.get::<_, Option<String>>(&cache_key).await {
                    let cached: CachedAuthConfig = serde_json::from_str(&cached)?;

                    return Ok(Some(AuthConfig {
                        app_id: cached.app_id,
                        app_user_id: cached.app_user_id,
                        app_name: cached.app_name,
                        app_description: None,
                        app_is_active: cached.app_is_active,
                        app_max_ttl_seconds: cached.app_max_ttl_seconds,
                        app_is_key_rotation_forced: cached.app_is_key_rotation_forced,
                        app_integrity_config: serde_json::json!({}),
                        sk_id: cached.sk_id,
                        sk_key_prefix: String::new(),
                        sk_tier: cached.sk_tier,
                        sk_monthly_message_quota: cached.sk_monthly_message_quota,
                        sk_message_retention_seconds: cached.sk_message_retention_seconds,
                        sk_rate_limit_per_minute: cached.sk_rate_limit_per_minute,
                    }));
                }
            }
        }

        // --- POSTGRES FALLBACK ---
        let auth = sqlx::query_as::<_, AuthConfig>(
            "SELECT * FROM fetch_auth_config_by_publishable_key($1)",
        )
        .bind(pk_plaintext)
        .fetch_optional(exec)
        .await?;

        // Cache if full record exists
        if let (Some(full), Some(redis_pool)) = (&auth, redis) {
            if let Ok(mut conn) = redis_pool.get().await {
                let cached: CachedAuthConfig = full.clone().into();
                let _ = conn
                    .set_ex::<_, _, ()>(&cache_key, serde_json::to_string(&cached)?, 3600)
                    .await;
            }
        }

        Ok(auth)
    }

    pub async fn fetch_auth_config_by_secret_hash<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        secret_hash_hex: &str,
    ) -> Result<Option<AuthConfig>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = secret_key_resolution_cache_key(secret_hash_hex);

        // --- HOT PATH ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(Some(cached)) = conn.get::<_, Option<String>>(&cache_key).await {
                    let cached: CachedAuthConfig = serde_json::from_str(&cached)?;

                    return Ok(Some(AuthConfig {
                        app_id: cached.app_id,
                        app_user_id: cached.app_user_id,
                        app_name: cached.app_name,
                        app_description: None,
                        app_is_active: cached.app_is_active,
                        app_max_ttl_seconds: cached.app_max_ttl_seconds,
                        app_is_key_rotation_forced: cached.app_is_key_rotation_forced,
                        app_integrity_config: serde_json::json!({}),
                        sk_id: cached.sk_id,
                        sk_key_prefix: String::new(),
                        sk_tier: cached.sk_tier,
                        sk_monthly_message_quota: cached.sk_monthly_message_quota,
                        sk_message_retention_seconds: cached.sk_message_retention_seconds,
                        sk_rate_limit_per_minute: cached.sk_rate_limit_per_minute,
                    }));
                }
            }
        }

        // --- POSTGRES FALLBACK ---
        let auth =
            sqlx::query_as::<_, AuthConfig>("SELECT * FROM fetch_auth_config_by_secret_hash($1)")
                .bind(secret_hash_hex)
                .fetch_optional(exec)
                .await?;

        // Cache if needed
        if let (Some(full), Some(redis_pool)) = (&auth, redis) {
            if let Ok(mut conn) = redis_pool.get().await {
                let cached: CachedAuthConfig = full.clone().into();
                let _ = conn
                    .set_ex::<_, _, ()>(&cache_key, serde_json::to_string(&cached)?, 3600)
                    .await;
            }
        }

        Ok(auth)
    }

    pub async fn fetch_full_auth_by_publishable_key<'c, E>(
        exec: E,
        pk_plaintext: &str,
    ) -> Result<Option<AuthConfig>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let auth = sqlx::query_as::<_, AuthConfig>(
            "SELECT * FROM fetch_auth_config_by_publishable_key($1)",
        )
        .bind(pk_plaintext)
        .fetch_optional(exec)
        .await?;

        Ok(auth)
    }

    pub async fn fetch_full_auth_by_secret_hash<'c, E>(
        exec: E,
        secret_hash_hex: &str,
    ) -> Result<Option<AuthConfig>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let auth =
            sqlx::query_as::<_, AuthConfig>("SELECT * FROM fetch_auth_config_by_secret_hash($1)")
                .bind(secret_hash_hex)
                .fetch_optional(exec)
                .await?;

        Ok(auth)
    }
}
