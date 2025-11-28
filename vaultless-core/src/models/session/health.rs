use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Health tracker for pub/sub connection
#[derive(Clone)]
pub struct PubSubHealth {
    /// Is pub/sub currently connected?
    connected: Arc<AtomicBool>,

    /// Timestamp of last received message (nanoseconds since epoch)
    last_message_ns: Arc<AtomicU64>,

    /// Total messages received
    messages_received: Arc<AtomicU64>,

    /// Total reconnection attempts
    reconnect_attempts: Arc<AtomicU64>,
}

impl PubSubHealth {
    pub fn new() -> Self {
        Self {
            connected: Arc::new(AtomicBool::new(false)),
            last_message_ns: Arc::new(AtomicU64::new(0)),
            messages_received: Arc::new(AtomicU64::new(0)),
            reconnect_attempts: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Mark pub/sub as connected
    pub fn mark_connected(&self) {
        self.connected.store(true, Ordering::SeqCst);
        super::metrics::PUBSUB_HEALTHY.set(1);
        tracing::info!("Pub/sub marked as connected");
    }

    /// Mark pub/sub as disconnected
    pub fn mark_disconnected(&self) {
        self.connected.store(false, Ordering::SeqCst);
        super::metrics::PUBSUB_HEALTHY.set(0);
        tracing::warn!("Pub/sub marked as disconnected");
    }

    /// Record that a message was received
    pub fn record_message(&self) {
        let now = Instant::now();
        self.last_message_ns
            .store(now.elapsed().as_nanos() as u64, Ordering::SeqCst);
        self.messages_received.fetch_add(1, Ordering::SeqCst);
    }

    /// Record a reconnection attempt
    pub fn record_reconnect(&self) {
        self.reconnect_attempts.fetch_add(1, Ordering::SeqCst);
        super::metrics::PUBSUB_RECONNECTS.inc();
    }

    /// Check if pub/sub is healthy
    ///
    /// Considers healthy if:
    /// 1. Connected = true
    /// 2. Received a message in last 2 minutes (heartbeat interval)
    pub fn is_healthy(&self, max_silence_duration: Duration) -> bool {
        if !self.connected.load(Ordering::SeqCst) {
            return false;
        }

        let last_msg_ns = self.last_message_ns.load(Ordering::SeqCst);
        if last_msg_ns == 0 {
            // No messages yet, but connected - give it some time
            return true;
        }

        let now_ns = Instant::now().elapsed().as_nanos() as u64;
        let silence_duration_ns = now_ns.saturating_sub(last_msg_ns);
        let silence_duration = Duration::from_nanos(silence_duration_ns);

        silence_duration < max_silence_duration
    }

    /// Get statistics
    pub fn stats(&self) -> HealthStats {
        HealthStats {
            connected: self.connected.load(Ordering::SeqCst),
            messages_received: self.messages_received.load(Ordering::SeqCst),
            reconnect_attempts: self.reconnect_attempts.load(Ordering::SeqCst),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HealthStats {
    pub connected: bool,
    pub messages_received: u64,
    pub reconnect_attempts: u64,
}

impl Default for PubSubHealth {
    fn default() -> Self {
        Self::new()
    }
}
