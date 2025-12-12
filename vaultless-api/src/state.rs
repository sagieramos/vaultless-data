use super::services::token::*;
use crate::services::cache::CacheService;
use crate::services::real_time_message::WsManager;
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;
use vaultless_core::AttestationService;
use vaultless_core::models::message::dto::InstantMessage;
use vaultless_core::models::session::{HybridSessionVerifier, SessionKeyManager, SessionVerifier};
use vaultless_core::models::usage::MetricsConfig;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<PgPool>,
    pub redis_pool: Arc<RedisPool>,
    pub token_service: Arc<TokenService>,
    pub instant_message: Arc<InstantMessage>,
    pub session_verifier_hybrid: Arc<HybridSessionVerifier>,
    pub _session_verifier: Arc<SessionVerifier>,
    pub ws_manager: Arc<WsManager>,
    pub attestation_service: Option<Arc<AttestationService>>,
}

impl AppState {
    pub fn new(
        db: PgPool,
        redis_pool: RedisPool,
        metrics_config: Arc<MetricsConfig>,
        redis_url: String,
        session_key_manager: Arc<SessionKeyManager>,
    ) -> anyhow::Result<Self> {
        let im_db_clone = db.clone();
        let im_redis_pool_clone = redis_pool.clone();

        let instant_message = Arc::new(InstantMessage::new(
            im_redis_pool_clone,
            im_db_clone,
            metrics_config,
        )?);

        let arc_db = Arc::new(db);
        let arc_redis_pool = Arc::new(redis_pool);

        let attestation_service = AttestationService::new(arc_redis_pool.clone(), arc_db.clone());

        let session_verifier_hybrid = Arc::new(HybridSessionVerifier::with_defaults(
            session_key_manager.clone(),
            arc_redis_pool.clone(),
            redis_url.clone(),
        ));

        let session_verifier = Arc::new(SessionVerifier::with_defaults(
            session_key_manager,
            arc_redis_pool.clone(),
        ));

        let token_service = Arc::new(TokenService::new(arc_db.clone(), arc_redis_pool.clone()));

        let ws_manager = WsManager::new(redis_url, Arc::clone(&instant_message));

        Ok(Self {
            db: arc_db,
            redis_pool: arc_redis_pool,
            token_service,
            instant_message,
            session_verifier_hybrid,
            _session_verifier,
            ws_manager,
            attestation_service: Some(Arc::new(attestation_service)),
        })
    }

    pub fn cache_service(&self) -> CacheService {
        CacheService::new(self.redis_pool.clone(), 3600)
    }
}
