use crate::error::{Result, VaultlessError};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,   // Normal operation
    Open,     // Failing, reject requests
    HalfOpen, // Testing if service recovered
}

/// Circuit breaker for Redis/DB operations
pub struct CircuitBreaker {
    state: Arc<AtomicU64>, // Packed: state (2 bits) + failure_count (30 bits) + last_failure_time (32 bits)
    failure_threshold: u32,
    timeout_seconds: u64,
    half_open_max_calls: u32,
    half_open_calls: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, timeout_seconds: u64) -> Self {
        Self {
            state: Arc::new(AtomicU64::new(0)), // Closed state
            failure_threshold,
            timeout_seconds,
            half_open_max_calls: 3,
            half_open_calls: AtomicU64::new(0),
        }
    }

    /// Check if request should be allowed
    pub fn allow_request(&self) -> Result<CircuitBreakerGuard<'_>> {
        let packed = self.state.load(Ordering::Acquire);
        let state = self.unpack_state(packed);

        match state {
            CircuitState::Closed => Ok(CircuitBreakerGuard { breaker: self }),
            CircuitState::Open => {
                // Check if timeout elapsed
                let last_failure = self.unpack_last_failure(packed);
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                if now - last_failure > self.timeout_seconds {
                    // Transition to half-open
                    self.transition_to_half_open();
                    Ok(CircuitBreakerGuard { breaker: self })
                } else {
                    Err(VaultlessError::CircuitBreakerOpen(
                        "Service temporarily unavailable".into(),
                    ))
                }
            }
            CircuitState::HalfOpen => {
                let calls = self.half_open_calls.fetch_add(1, Ordering::SeqCst);
                if calls < self.half_open_max_calls as u64 {
                    Ok(CircuitBreakerGuard { breaker: self })
                } else {
                    Err(VaultlessError::CircuitBreakerOpen(
                        "Half-open limit exceeded".into(),
                    ))
                }
            }
        }
    }

    fn record_success(&self) {
        let packed = self.state.load(Ordering::Acquire);
        let state = self.unpack_state(packed);

        match state {
            CircuitState::HalfOpen => {
                // Transition back to closed
                self.state.store(0, Ordering::Release);
                self.half_open_calls.store(0, Ordering::Release);
                info!("Circuit breaker closed - service recovered");
            }
            CircuitState::Closed => {
                // Reset failure count on success
                let new_packed = self.pack_state(CircuitState::Closed, 0, 0);
                self.state.store(new_packed, Ordering::Release);
            }
            _ => {}
        }
    }

    fn record_failure(&self) {
        let packed = self.state.load(Ordering::Acquire);
        let state = self.unpack_state(packed);
        let failure_count = self.unpack_failure_count(packed);

        let new_count = failure_count + 1;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if new_count >= self.failure_threshold {
            // Trip the circuit breaker
            let new_packed = self.pack_state(CircuitState::Open, new_count, now);
            self.state.store(new_packed, Ordering::Release);
            error!(
                failure_count = new_count,
                "Circuit breaker opened - too many failures"
            );
        } else {
            let new_packed = self.pack_state(state, new_count, now);
            self.state.store(new_packed, Ordering::Release);
        }
    }

    fn transition_to_half_open(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let new_packed = self.pack_state(CircuitState::HalfOpen, 0, now);
        self.state.store(new_packed, Ordering::Release);
        self.half_open_calls.store(0, Ordering::Release);
        info!("Circuit breaker half-open - testing service");
    }

    // Pack/unpack helpers
    fn pack_state(&self, state: CircuitState, failure_count: u32, last_failure: u64) -> u64 {
        let state_bits = match state {
            CircuitState::Closed => 0u64,
            CircuitState::Open => 1u64,
            CircuitState::HalfOpen => 2u64,
        };
        (state_bits << 62) | ((failure_count as u64) << 32) | (last_failure & 0xFFFFFFFF)
    }

    fn unpack_state(&self, packed: u64) -> CircuitState {
        match packed >> 62 {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }

    fn unpack_failure_count(&self, packed: u64) -> u32 {
        ((packed >> 32) & 0x3FFFFFFF) as u32
    }

    fn unpack_last_failure(&self, packed: u64) -> u64 {
        packed & 0xFFFFFFFF
    }

    pub fn get_state(&self) -> CircuitState {
        let packed = self.state.load(Ordering::Acquire);
        self.unpack_state(packed)
    }
}

/// RAII guard for circuit breaker
pub struct CircuitBreakerGuard<'a> {
    breaker: &'a CircuitBreaker,
}

impl<'a> CircuitBreakerGuard<'a> {
    pub fn success(self) {
        self.breaker.record_success();
    }

    pub fn failure(self) {
        self.breaker.record_failure();
    }
}
