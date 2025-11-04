use axum::extract::FromRef;
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;

use crate::config::Config;
use crate::services::cache::CacheService;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<PgPool>,
    pub redis_pool: Arc<RedisPool>,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(db: PgPool, redis_pool: RedisPool, config: Config) -> Self {
        Self {
            db: Arc::new(db),
            redis_pool: Arc::new(redis_pool),
            config: Arc::new(config),
        }
    }

    pub fn cache_service(&self) -> CacheService {
        CacheService::new(self.redis_pool.clone(), 3600)
    }
}
