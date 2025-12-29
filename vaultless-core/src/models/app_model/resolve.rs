use super::dto::*;
use crate::error::Result;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::collections::HashMap;
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

        // Note: We always fetch from DB because callers may need app_meta for
        // integrity checks. The cache is only used by fetch_and_validate_hot()
        // for quota/rate limit checks in the hot path.

        // --- POSTGRES QUERY ---
        let auth = sqlx::query_as::<_, ApplicationKeyView>(
            "SELECT * FROM fetch_auth_config_by_publishable_key($1)",
        )
        .bind(pk_plaintext)
        .fetch_optional(exec)
        .await?;

        // Cache-Aside: Update Redis HASH if found and app is active
        if let (Some(full), Some(redis_pool)) = (&auth, redis) {
            if full.app_is_active {
                if let Ok(mut conn) = redis_pool.get().await {
                    let auth_entry: AuthCacheEntry = full.clone().into();
                    let args = auth_entry.to_redis_args();
                    // Use HMSET command with all arguments
                    let mut cmd = redis::cmd("HMSET");
                    cmd.arg(&cache_key);
                    for arg in &args {
                        cmd.arg(arg);
                    }
                    let _: () = cmd.query_async(&mut *conn).await?;
                    // Set TTL on the HASH key
                    let _: () = redis::cmd("EXPIRE")
                        .arg(&cache_key)
                        .arg(AuthCacheEntry::TTL_SECONDS)
                        .query_async(&mut *conn)
                        .await?;
                }
            }
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

        // Note: We always fetch from DB because callers may need app_meta for
        // integrity checks. The cache is only used by fetch_and_validate_hot()
        // for quota/rate limit checks in the hot path.

        // --- POSTGRES QUERY ---
        let auth = sqlx::query_as::<_, ApplicationKeyView>(
            "SELECT * FROM fetch_auth_config_by_secret_hash($1)",
        )
        .bind(secret_hash_hex)
        .fetch_optional(exec)
        .await?;

        // Cache-Aside: Update Redis HASH if found and app is active
        if let (Some(full), Some(redis_pool)) = (&auth, redis) {
            if full.app_is_active {
                if let Ok(mut conn) = redis_pool.get().await {
                    let auth_entry: AuthCacheEntry = full.clone().into();
                    let args = auth_entry.to_redis_args();
                    // Use HMSET command with all arguments
                    let mut cmd = redis::cmd("HMSET");
                    cmd.arg(&cache_key);
                    for arg in &args {
                        cmd.arg(arg);
                    }
                    let _: () = cmd.query_async(&mut *conn).await?;
                    // Set TTL on the HASH key
                    let _: () = redis::cmd("EXPIRE")
                        .arg(&cache_key)
                        .arg(AuthCacheEntry::TTL_SECONDS)
                        .query_async(&mut *conn)
                        .await?;
                }
            }
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