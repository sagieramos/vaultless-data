use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;

use crate::config::Config;
use crate::services::cache::CacheService;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cache: RedisPool,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(db: PgPool, cache: RedisPool, config: Config) -> Self {
        Self {
            db,
            cache,
            config: Arc::new(config),
        }
    }

    pub fn cache_service(&self) -> CacheService {
        CacheService::new(self.cache.clone(), 3600)
    }
}
