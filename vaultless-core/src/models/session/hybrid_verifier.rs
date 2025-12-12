// vaultless-core/src/models/session/hybrid_verifier.rs
use super::health::PubSubHealth;
use super::metrics;
use super::paseto_session::{SessionData, SessionKeyManager, verify_session_token};
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use futures_util::StreamExt;
use moka::future::Cache;
use redis::{AsyncCommands, Client as RedisClient};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const CHANNEL_NAME: &str = "session:invalidate";
const HEARTBEAT_CHANNEL: &str = "session:heartbeat";
const HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Configuration for hybrid session verifier
#[derive(Debug, Clone)]
pub struct HybridVerifierConfig {
    /// Maximum number of entries in local cache
    pub cache_size: u64,
    /// Time-to-live for cached entries (seconds)
    pub cache_ttl_seconds: u64,
    /// Maximum silence duration before considering pub/sub unhealthy (seconds)
    pub max_silence_seconds: u64,
    /// Enable fallback to Redis when pub/sub is unhealthy
    pub enable_fallback: bool,
    /// Redis connection URL for pub/sub
    pub redis_url: String,
}

impl Default for HybridVerifierConfig {
    fn default() -> Self {
        Self {
            cache_size: 10_000,
            cache_ttl_seconds: 60,
            max_silence_seconds: 120,
            enable_fallback: true,
            redis_url: "redis://127.0.0.1:6379/".to_string(),
        }
    }
}

/// Production-ready hybrid session verifier with local cache + Redis pub/sub
pub struct HybridSessionVerifier {
    key_manager: Arc<SessionKeyManager>,
    redis_pool: Arc<RedisPool>,
    local_cache: Cache<String, bool>,
    health: PubSubHealth,
    config: HybridVerifierConfig,
    pubsub_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl HybridSessionVerifier {
    /// Create new hybrid verifier with custom configuration
    pub fn new(
        key_manager: Arc<SessionKeyManager>,
        redis_pool: Arc<RedisPool>,
        config: HybridVerifierConfig,
    ) -> Self {
        let local_cache = Cache::builder()
            .max_capacity(config.cache_size)
            .time_to_live(Duration::from_secs(config.cache_ttl_seconds))
            .build();
        let health = PubSubHealth::new();
        let redis_url = config.redis_url.clone();
        let verifier = Self {
            key_manager,
            redis_pool: redis_pool.clone(),
            local_cache,
            health: health.clone(),
            config: config.clone(),
            pubsub_handle: Arc::new(RwLock::new(None)),
        };
        // Start pub/sub listener
        let handle = tokio::spawn(Self::run_pubsub_listener(
            redis_url,
            redis_pool,
            verifier.local_cache.clone(),
            health,
        ));
        // Use try_write to avoid blocking in potentially concurrent context
        if let Ok(mut guard) = verifier.pubsub_handle.try_write() {
            *guard = Some(handle);
        } else {
            tracing::warn!("Failed to set pubsub handle — ensure manual abort on drop");
        }
        verifier
    }

    /// Create with default configuration (requires Redis URL)
    pub fn with_defaults(
        key_manager: Arc<SessionKeyManager>,
        redis_pool: Arc<RedisPool>,
        redis_url: String,
    ) -> Self {
        let mut config = HybridVerifierConfig::default();
        config.redis_url = redis_url;
        Self::new(key_manager, redis_pool, config)
    }

    /// Verify session with hybrid caching (PRIMARY METHOD - USE THIS)
    pub async fn verify_fast(&self, token: &str) -> Result<SessionData> {
        let start = Instant::now();
        let (session_data, jti) = verify_session_token(&self.key_manager, token)?;
        // Check local cache first
        if let Some(is_valid) = self.local_cache.get(&jti).await {
            metrics::CACHE_HITS.inc();
            // SAFETY CHECK: If pub/sub is unhealthy, verify against Redis
            if self.config.enable_fallback && !self.is_pubsub_healthy() {
                tracing::warn!(
                    jti = %jti,
                    "Pub/sub unhealthy, verifying cached result against Redis"
                );
                metrics::FALLBACK_TO_REDIS.inc();
                let is_revoked = self.check_revocation_redis(&jti).await?;
                // Detect stale cache: cached 'is_valid' should be !is_revoked
                if is_revoked == is_valid {
                    metrics::STALE_CACHE_DETECTED.inc();
                    tracing::error!(
                        jti = %jti,
                        cached_valid = is_valid,
                        redis_revoked = is_revoked,
                        duration_ms = start.elapsed().as_millis(),
                        "Stale cache detected! Invalidating."
                    );
                    self.local_cache.invalidate(&jti).await;
                    if is_revoked {
                        metrics::VERIFY_DURATION.observe(start.elapsed().as_secs_f64());
                        return Err(VaultlessError::Unauthorized("Session revoked".into()));
                    }
                }
            }
            if !is_valid {
                metrics::VERIFY_DURATION.observe(start.elapsed().as_secs_f64());
                return Err(VaultlessError::Unauthorized("Session revoked".into()));
            }
            metrics::VERIFY_DURATION.observe(start.elapsed().as_secs_f64());
            return Ok(session_data);
        }
        // Cache miss - check Redis
        metrics::CACHE_MISSES.inc();
        let is_revoked = self.check_revocation_redis(&jti).await?;
        // Populate cache
        self.local_cache.insert(jti, !is_revoked).await;
        if is_revoked {
            metrics::VERIFY_DURATION.observe(start.elapsed().as_secs_f64());
            return Err(VaultlessError::Unauthorized("Session revoked".into()));
        }
        metrics::VERIFY_DURATION.observe(start.elapsed().as_secs_f64());
        Ok(session_data)
    }

    /// Verify session without local cache (always checks Redis)
    pub async fn verify_secure(&self, token: &str) -> Result<SessionData> {
        let start = Instant::now();
        let (session_data, jti) = verify_session_token(&self.key_manager, token)?;
        metrics::REDIS_CHECKS.inc();
        let is_revoked = self.check_revocation_redis(&jti).await?;
        if is_revoked {
            metrics::VERIFY_DURATION.observe(start.elapsed().as_secs_f64());
            return Err(VaultlessError::Unauthorized("Session revoked".into()));
        }
        metrics::VERIFY_DURATION.observe(start.elapsed().as_secs_f64());
        Ok(session_data)
    }

    /// Revoke a session and broadcast to all nodes
    pub async fn revoke_session(&self, jti: &str, remaining_ttl_seconds: u64) -> Result<()> {
        let revoked_key = cache_key!("revoked_session", jti);
        let mut conn = self
            .redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {e}")))?;
        // Mark as revoked in Redis
        conn.set_ex::<_, _, ()>(&revoked_key, "1", remaining_ttl_seconds)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis SETEX failed: {e}")))?;
        // Invalidate local cache
        self.local_cache.invalidate(jti).await;
        // Broadcast invalidation to all nodes
        let published: i32 = conn
            .publish(CHANNEL_NAME, jti)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis PUBLISH failed: {e}")))?;
        metrics::INVALIDATIONS_SENT.inc();
        tracing::info!(
            jti = %jti,
            ttl = remaining_ttl_seconds,
            subscribers = published,
            "Session revoked and broadcasted to {} nodes",
            published
        );
        Ok(())
    }

    pub fn key_manager(&self) -> &Arc<SessionKeyManager> {
        &self.key_manager
    }

    /// Check if pub/sub is healthy
    pub fn is_pubsub_healthy(&self) -> bool {
        self.health
            .is_healthy(Duration::from_secs(self.config.max_silence_seconds))
    }

    /// Get health statistics
    pub fn health_stats(&self) -> super::health::HealthStats {
        self.health.stats()
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            entry_count: self.local_cache.entry_count(),
            weighted_size: self.local_cache.weighted_size(),
            hit_rate: metrics::CACHE_HITS.get() as f64
                / (metrics::CACHE_HITS.get() + metrics::CACHE_MISSES.get()).max(1) as f64,
        }
    }

    /// Clear local cache (useful for testing or forced refresh)
    pub async fn clear_cache(&self) {
        self.local_cache.invalidate_all();
        self.local_cache.run_pending_tasks().await;
        tracing::info!("Local cache cleared");
    }

    /// Check revocation status in Redis
    async fn check_revocation_redis(&self, jti: &str) -> Result<bool> {
        let key = cache_key!("revoked_session", jti);
        let mut conn = self
            .redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {e}")))?;
        metrics::REDIS_CHECKS.inc();
        let revoked: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis GET failed: {e}")))?;
        Ok(revoked.is_some())
    }

    /// Background task: Pub/sub listener with automatic reconnection
    async fn run_pubsub_listener(
        redis_url: String,
        redis_pool: Arc<RedisPool>,
        local_cache: Cache<String, bool>,
        health: PubSubHealth,
    ) {
        tracing::info!("Starting pub/sub listener");
        loop {
            health.mark_disconnected();
            health.record_reconnect();
            match Self::establish_pubsub_connection(&redis_url, redis_pool.clone()).await {
                Ok((mut pubsub, heartbeat_handle)) => {
                    health.mark_connected();
                    tracing::info!("Pub/sub connection established");
                    // Listen for messages
                    loop {
                        match pubsub.on_message().next().await {
                            Some(msg) => {
                                health.record_message();
                                if let Ok(payload) = msg.get_payload::<String>() {
                                    // Ignore heartbeat messages
                                    if payload == "ping" {
                                        continue;
                                    }
                                    // Process invalidation message
                                    local_cache.invalidate(&payload).await;
                                    metrics::INVALIDATIONS_RECEIVED.inc();
                                    tracing::debug!(
                                        jti = %payload,
                                        "Received invalidation message"
                                    );
                                }
                            }
                            None => {
                                tracing::warn!("Pub/sub connection lost");
                                health.mark_disconnected();
                                heartbeat_handle.abort();
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to establish pub/sub connection: {}", e);
                }
            }
            // Wait before reconnecting
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    /// Establish pub/sub connection and subscribe to channels
    async fn establish_pubsub_connection(
        redis_url: &str,
        redis_pool: Arc<RedisPool>,
    ) -> Result<(redis::aio::PubSub, tokio::task::JoinHandle<()>)> {
        let client = RedisClient::open(redis_url)
            .map_err(|e| VaultlessError::Internal(format!("Failed to create Redis client: {e}")))?;
        // Use get_async_pubsub() directly
        let mut pubsub = client.get_async_pubsub().await.map_err(|e| {
            VaultlessError::Internal(format!("Failed to get pub/sub connection: {e}"))
        })?;
        // Subscribe to channels...
        pubsub
            .subscribe(CHANNEL_NAME)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Failed to subscribe: {e}")))?;
        pubsub.subscribe(HEARTBEAT_CHANNEL).await.map_err(|e| {
            VaultlessError::Internal(format!("Failed to subscribe to heartbeat: {e}"))
        })?;
        // Start heartbeat...
        let heartbeat_handle = tokio::spawn(async move {
            Self::send_heartbeat(redis_pool).await;
        });
        Ok((pubsub, heartbeat_handle))
    }

    /// Background task: Send periodic heartbeat
    async fn send_heartbeat(redis_pool: Arc<RedisPool>) {
        loop {
            tokio::time::sleep(Duration::from_secs(HEARTBEAT_INTERVAL_SECS)).await;
            match redis_pool.get().await {
                Ok(mut conn) => {
                    if let Err(e) = conn.publish::<_, _, ()>(HEARTBEAT_CHANNEL, "ping").await {
                        tracing::error!("Failed to send heartbeat: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to get connection for heartbeat: {}", e);
                }
            }
        }
    }
}

/// Graceful shutdown
impl Drop for HybridSessionVerifier {
    fn drop(&mut self) {
        if let Some(handle) = self.pubsub_handle.blocking_write().take() {
            handle.abort();
            tracing::info!("Pub/sub listener stopped");
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entry_count: u64,
    pub weighted_size: u64,
    pub hit_rate: f64,
}
