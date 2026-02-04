use super::dto::*;
use crate::error::Result;
use deadpool_redis::Pool as RedisPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

pub enum AuthLookup<'a> {
    Publishable(&'a str),
    SecretHash(&'a str),
}

impl<'a> AuthLookup<'a> {
    fn sql_function(&self) -> &'static str {
        match self {
            AuthLookup::Publishable(_) => "fetch_auth_config_by_publishable_key",
            AuthLookup::SecretHash(_) => "fetch_auth_config_by_secret_hash",
        }
    }

    fn value(&self) -> &str {
        match self {
            AuthLookup::Publishable(v) => v,
            AuthLookup::SecretHash(v) => v,
        }
    }

    fn cache_key(&self) -> String {
        match self {
            AuthLookup::Publishable(v) => publishable_key_resolution_cache_key(v),
            AuthLookup::SecretHash(v) => secret_key_resolution_cache_key(v),
        }
    }
}

impl Application {
    pub async fn fetch_auth_internal<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        lookup: AuthLookup<'_>,
        use_cache: bool,
    ) -> Result<Option<ApplicationKeyView>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let sql = format!("SELECT * FROM {}($1)", lookup.sql_function());

        let auth = sqlx::query_as::<_, ApplicationKeyView>(&sql)
            .bind(lookup.value())
            .fetch_optional(exec)
            .await?;

        // --- Cache Aside ---
        if use_cache {
            if let (Some(full), Some(redis_pool)) = (&auth, redis) {
                if full.app_is_active {
                    if let Ok(mut conn) = redis_pool.get().await {
                        let cache_key = lookup.cache_key();

                        let auth_entry: AuthCacheEntry = full.clone().into();
                        let args = auth_entry.to_redis_args();

                        let mut cmd = redis::cmd("HMSET");
                        cmd.arg(&cache_key);

                        for arg in &args {
                            cmd.arg(arg);
                        }

                        let _: () = cmd.query_async(&mut *conn).await?;

                        let _: () = redis::cmd("EXPIRE")
                            .arg(&cache_key)
                            .arg(AuthCacheEntry::TTL_SECONDS)
                            .query_async(&mut *conn)
                            .await?;
                    }
                }
            }
        }

        Ok(auth)
    }
}
