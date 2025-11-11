use super::dto::*;
use crate::error::Result;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;

impl Application {
    /// Invalidate application cache (standard method - requires full Application)
    pub async fn invalidate_cache(redis: Option<Arc<RedisPool>>, app: &Application) {
        if let Some(redis_pool) = redis {
            let app_id = app.id;
            let publishable_key = app.publishable_key.clone();

            tokio::spawn(async move {
                if let Ok(mut conn) = redis_pool.get().await {
                    let id_key = cache_key_by_id(app_id);
                    let pk_key = cache_key_by_publishable_key(&publishable_key);

                    let _ = conn.del::<_, ()>(&id_key).await;
                    let _ = conn.del::<_, ()>(&pk_key).await;

                    tracing::debug!(
                        app_id = %app_id,
                        "Invalidated application cache"
                    );
                }
            });
        }
    }

    /// Invalidate cache by ID only (useful when you don't have the full Application object)
    pub async fn invalidate_cache_by_id<'c, E>(
        redis: Option<Arc<RedisPool>>,
        exec: E,
        app_id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let publishable_key: Option<String> =
            sqlx::query_scalar("SELECT publishable_key FROM applications WHERE id = $1")
                .bind(app_id)
                .fetch_optional(exec)
                .await?;

        if let Some(pk) = publishable_key {
            if let Some(redis_pool) = redis {
                tokio::spawn(async move {
                    if let Ok(mut conn) = redis_pool.get().await {
                        let id_key = cache_key_by_id(app_id);
                        let pk_key = cache_key_by_publishable_key(&pk);

                        let _ = conn.del::<_, ()>(&id_key).await;
                        let _ = conn.del::<_, ()>(&pk_key).await;

                        tracing::debug!(
                            app_id = %app_id,
                            "Invalidated application cache by ID"
                        );
                    }
                });
            }
        }

        Ok(())
    }

    /// Batch invalidate caches for multiple applications (more efficient than individual calls)
    pub async fn invalidate_caches_batch(redis: Option<Arc<RedisPool>>, apps: &[Application]) {
        if apps.is_empty() {
            return;
        }

        if let Some(redis_pool) = redis {
            let cache_keys: Vec<String> = apps
                .iter()
                .flat_map(|app| {
                    vec![
                        cache_key_by_id(app.id),
                        cache_key_by_publishable_key(&app.publishable_key),
                    ]
                })
                .collect();

            let count = apps.len();

            tokio::spawn(async move {
                if let Ok(mut conn) = redis_pool.get().await {
                    // Use Redis pipeline for efficiency
                    let mut pipe = redis::pipe();
                    for key in &cache_keys {
                        pipe.del(key);
                    }

                    if let Err(e) = pipe.query_async::<()>(&mut *conn).await {
                        tracing::warn!(
                            error = %e,
                            count = count,
                            "Failed to batch invalidate application caches"
                        );
                    } else {
                        tracing::debug!(count = count, "Batch invalidated application caches");
                    }
                }
            });
        }
    }
}
