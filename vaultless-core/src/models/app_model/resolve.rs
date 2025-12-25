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

        // --- HOT PATH (Redis HASH) ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                // HGETALL for O(1) field access without JSON parsing
                if let Ok(vals) = conn.hgetall::<_, HashMap<String, String>>(&cache_key).await {
                    if !vals.is_empty() {
                        if let Some(auth_entry) = AuthCacheEntry::from_redis(vals) {
                            // Only return if app is active
                            if auth_entry.is_active {
                                return Ok(Some(auth_entry.into_application_key_view()));
                            }
                            // Cache exists but app is inactive - return None
                            return Ok(None);
                        }
                    }
                }
            }
        }

        // --- POSTGRES FALLBACK ---
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

        // --- HOT PATH (Redis HASH) ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                // HGETALL for O(1) field access without JSON parsing
                if let Ok(vals) = conn.hgetall::<_, HashMap<String, String>>(&cache_key).await {
                    if !vals.is_empty() {
                        if let Some(auth_entry) = AuthCacheEntry::from_redis(vals) {
                            // Only return if app is active
                            if auth_entry.is_active {
                                return Ok(Some(auth_entry.into_application_key_view()));
                            }
                            // Cache exists but app is inactive - return None
                            return Ok(None);
                        }
                    }
                }
            }
        }

        // --- POSTGRES FALLBACK ---
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