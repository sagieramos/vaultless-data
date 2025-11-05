// vaultless-api/src/state.rs
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;

use crate::config::Config;
use crate::services::cache::CacheService;
use vaultless_core::models::instant_message::InstantMessage;
use vaultless_core::models::usage::MetricsConfig;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<PgPool>,
    pub redis_pool: Arc<RedisPool>,
    pub config: Arc<Config>,
    pub instant_message: Arc<InstantMessage>,
}

impl AppState {
    pub fn new(db: PgPool, redis_pool: RedisPool, config: Config) -> anyhow::Result<Self> {
        // Create metrics config with actual MetricsConfig fields
        let metrics_config = MetricsConfig {
            max_batch_size: config.metrics_max_batch_size.unwrap_or(1000),
            metric_ttl_secs: config.metrics_ttl_secs.unwrap_or(2592000), // 30 days default
            flush_interval_secs: config.metrics_flush_interval_secs.unwrap_or(60),
            redis_operation_timeout_secs: config.metrics_redis_timeout_secs.unwrap_or(5),
        };

        // Initialize InstantMessage service
        let instant_message = InstantMessage::new(redis_pool.clone(), db.clone(), metrics_config)?;

        Ok(Self {
            db: Arc::new(db),
            redis_pool: Arc::new(redis_pool),
            config: Arc::new(config),
            instant_message: Arc::new(instant_message),
        })
    }

    pub fn cache_service(&self) -> CacheService {
        CacheService::new(self.redis_pool.clone(), 3600)
    }
}