use super::dto::*;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

impl Application {
    /// Invalidate application cache (standard method - requires full Application)
    pub async fn invalidate_cache<'c, E>(
        exec: E,
        redis_pool: Arc<RedisPool>,
        app: &Application,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let secret_key_id = app.secret_key_id;
        let (sk_hash, pk_plaintext) =
            ApiKey::get_linked_key_data_for_cache_invalidation(exec.clone(), app.id).await?;

        let mut keys_to_delete = Vec::with_capacity(3);
        keys_to_delete.push(cache_key!("app_bundle", secret_key_id));
        keys_to_delete.push(publishable_key_resolution_cache_key(&pk_plaintext));

        if let Some(hash) = sk_hash {
            keys_to_delete.push(secret_key_resolution_cache_key(&hash));
        }

        conn.del::<_, u64>(keys_to_delete).await.map_err(|e| {
            tracing::error!("Redis DEL command failed: {:?}", e);
            VaultlessError::Internal("Failed to delete Redis cache keys".into())
        })?;

        tracing::info!(application_id = %app.id, "Application and all associated key caches fully invalidated.");

        Ok(())
    }
}
