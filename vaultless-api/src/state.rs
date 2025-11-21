// vaultless-api/src/state.rs
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;

use crate::services::cache::CacheService;
use vaultless_core::SessionKeyManager;
use vaultless_core::models::instant_message::InstantMessage;
use vaultless_core::models::usage::MetricsConfig;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<PgPool>,
    pub redis_pool: Arc<RedisPool>,
    pub session_key_manager: Arc<SessionKeyManager>,
    pub instant_message: Arc<InstantMessage>,
}

impl AppState {
    pub fn new(
        db: PgPool,
        redis_pool: RedisPool,
        metrics_config: MetricsConfig,
        session_key_manager: SessionKeyManager,
    ) -> anyhow::Result<Self> {
        let instant_message = InstantMessage::new(redis_pool, db, metrics_config)?;

        Ok(Self {
            db: Arc::new(db),
            redis_pool: Arc::new(redis_pool),
            session_key_manager: Arc::new(session_key_manager),
            instant_message: Arc::new(instant_message),
        })
    }

    pub fn cache_service(&self) -> CacheService {
        CacheService::new(self.redis_pool, 3600)
    }
}
