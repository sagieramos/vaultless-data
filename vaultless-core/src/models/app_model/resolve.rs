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
    ) -> Result<Option<ApplicationKeyView>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = publishable_key_resolution_cache_key(pk_plaintext);

        // --- HOT PATH (Redis) ---
        if let Some(redis_pool) = &redis
            && let Ok(mut conn) = redis_pool.get().await
            && let Ok(Some(cached_str)) = conn.get::<_, Option<String>>(&cache_key).await
        {
            let cached: CachedApplicationKeyView = serde_json::from_str(&cached_str)?;

            return Ok(Some(ApplicationKeyView {
                app_id: cached.app_id,
                app_user_id: cached.app_user_id,
                app_name: cached.app_name,
                app_description: None,
                app_is_active: cached.app_is_active,
                app_max_ttl_seconds: cached.app_max_ttl_seconds,
                app_is_key_rotation_forced: cached.app_is_key_rotation_forced,
                app_app_meta: serde_json::json!({}),
                sk_id: cached.sk_id,
                sk_key_prefix: String::new(),
                sub_tier: cached.sub_tier,
                sub_monthly_message_quota: cached.sub_monthly_message_quota,
                sub_message_retention_seconds: cached.sub_message_retention_seconds,
                sub_rate_limit_per_minute: cached.sub_rate_limit_per_minute,
            }));
        }

        // --- POSTGRES FALLBACK ---
        let auth = sqlx::query_as::<_, ApplicationKeyView>(
            "SELECT * FROM fetch_auth_config_by_publishable_key($1)",
        )
        .bind(pk_plaintext)
        .fetch_optional(exec)
        .await?;

        // Cache-Aside: Update Redis if found
        if let (Some(full), Some(redis_pool)) = (&auth, redis)
            && let Ok(mut conn) = redis_pool.get().await
        {
            let cached: CachedApplicationKeyView = full.clone().into();
            let _ = conn
                .set_ex::<_, _, ()>(&cache_key, serde_json::to_string(&cached)?, 3600)
                .await;
        }

        Ok(auth)
    }

    pub async fn fetch_auth_config_by_secret_hash<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        secret_hash_hex: &str,
    ) -> Result<Option<ApplicationKeyView>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = secret_key_resolution_cache_key(secret_hash_hex);

        // --- HOT PATH (Redis) ---
        if let Some(redis_pool) = &redis
            && let Ok(mut conn) = redis_pool.get().await
            && let Ok(Some(cached_str)) = conn.get::<_, Option<String>>(&cache_key).await
        {
            let cached: CachedApplicationKeyView = serde_json::from_str(&cached_str)?;

            return Ok(Some(ApplicationKeyView {
                app_id: cached.app_id,
                app_user_id: cached.app_user_id,
                app_name: cached.app_name,
                app_description: None,
                app_is_active: cached.app_is_active,
                app_max_ttl_seconds: cached.app_max_ttl_seconds,
                app_is_key_rotation_forced: cached.app_is_key_rotation_forced,
                app_app_meta: serde_json::json!({}),
                sk_id: cached.sk_id,
                sk_key_prefix: String::new(),
                // Map Subscription fields
                sub_tier: cached.sub_tier,
                sub_monthly_message_quota: cached.sub_monthly_message_quota,
                sub_message_retention_seconds: cached.sub_message_retention_seconds,
                sub_rate_limit_per_minute: cached.sub_rate_limit_per_minute,
            }));
        }

        // --- POSTGRES FALLBACK ---
        let auth = sqlx::query_as::<_, ApplicationKeyView>(
            "SELECT * FROM fetch_auth_config_by_secret_hash($1)",
        )
        .bind(secret_hash_hex)
        .fetch_optional(exec)
        .await?;

        if let (Some(full), Some(redis_pool)) = (&auth, redis)
            && let Ok(mut conn) = redis_pool.get().await
        {
            let cached: CachedApplicationKeyView = full.clone().into();
            let _ = conn
                .set_ex::<_, _, ()>(&cache_key, serde_json::to_string(&cached)?, 3600)
                .await;
        }

        Ok(auth)
    }

    pub async fn fetch_full_auth_by_publishable_key<'c, E>(
        exec: E,
        pk_plaintext: &str,
    ) -> Result<Option<ApplicationKeyView>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Ok(sqlx::query_as::<_, ApplicationKeyView>(
            "SELECT * FROM fetch_auth_config_by_publishable_key($1)",
        )
        .bind(pk_plaintext)
        .fetch_optional(exec)
        .await?)
    }

    pub async fn fetch_full_auth_by_secret_hash<'c, E>(
        exec: E,
        secret_hash_hex: &str,
    ) -> Result<Option<ApplicationKeyView>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Ok(sqlx::query_as::<_, ApplicationKeyView>(
            "SELECT * FROM fetch_auth_config_by_secret_hash($1)",
        )
        .bind(secret_hash_hex)
        .fetch_optional(exec)
        .await?)
    }
}