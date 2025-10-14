use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;

use crate::config::Config;

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
}
