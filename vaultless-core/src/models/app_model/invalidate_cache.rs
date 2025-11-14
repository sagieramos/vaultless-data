use super::dto::*;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

impl Application {
    /// Invalidate cached auth entries (SK and PK) for this application
    pub async fn invalidate_auth_cache<'c, E>(
        &self,
        exec: E,
        redis: Arc<RedisPool>,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // 1. Fetch all keys for this app (ignore is_active)
        let keys: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT key_hash, publishable_key_plaintext
            FROM api_keys
            WHERE application_id = $1
            "#,
        )
        .bind(self.id)
        .fetch_all(exec)
        .await
        .map_err(VaultlessError::Database)?;

        let mut conn = redis.get().await?;

        // 2. Delete each key from Redis
        for (sk_opt, pk_opt) in keys {
            if let Some(sk) = sk_opt {
                let sk_key = secret_key_resolution_cache_key(&sk);
                let _: () = conn.del(sk_key).await?;
            }
            if let Some(pk) = pk_opt {
                let pk_key = publishable_key_resolution_cache_key(&pk);
                let _: () = conn.del(pk_key).await?;
            }
        }

        Ok(())
    }
}
