use deadpool_redis::PoolError as DeadpoolRedisPoolError;
use redis::RedisError;
use serde_json::Error as SerdeJsonError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultlessError {
    // =========================================================================
    // Database errors
    // =========================================================================
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Duplicate record: {0}")]
    Duplicate(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    // =========================================================================
    // Validation errors
    // =========================================================================
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Missing required field: {0}")]
    MissingRequiredField(String),

    /// NEW: Integrity check failed due to invalid platform configuration (e.g., bad origin/token)
    #[error("Integrity check failed: {0}")]
    IntegrityCheckFailed(String),

    // =========================================================================
    // Authentication / Authorization errors
    // =========================================================================
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Email not verified")]
    EmailNotVerified(Option<String>),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Invalid API key")]
    InvalidApiKey,

    #[error("API key expired")]
    ApiKeyExpired,

    #[error("API key inactive")]
    ApiKeyInactive,

    // =========================================================================
    // Business logic errors
    // =========================================================================
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    #[error("Message expired")]
    MessageExpired,

    #[error("Message access limit reached")]
    MessageAccessLimitReached,

    // =========================================================================
    // Cryptography errors
    // =========================================================================
    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption error: {0}")]
    Decryption(String),

    #[error("Signing error: {0}")]
    Signing(String),

    #[error("Signature verification failed")]
    SignatureVerificationFailed(String),

    #[error("Invalid proof")]
    InvalidProof,

    // =========================================================================
    // Timeout errors
    // =========================================================================
    #[error("Operation timed out: {0}")]
    Timeout(String),

    // =========================================================================
    // Generic errors
    // =========================================================================
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Metrics increment failed")]
    MetricsIncrementFailed(String),
    
    #[error("Circuit breaker open")]
    CircuitBreakerOpen(String),
}

pub type Result<T> = std::result::Result<T, VaultlessError>;

impl VaultlessError {
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            VaultlessError::Validation(_)
                | VaultlessError::BadRequest(_)
                | VaultlessError::InvalidInput(_)
                | VaultlessError::MissingRequiredField(_)
                | VaultlessError::IntegrityCheckFailed(_) // ⭐ ADDED HERE ⭐
                | VaultlessError::Unauthorized(_)
                | VaultlessError::EmailNotVerified(_)
                | VaultlessError::Forbidden(_)
                | VaultlessError::InvalidApiKey
                | VaultlessError::ApiKeyExpired
                | VaultlessError::ApiKeyInactive
                | VaultlessError::QuotaExceeded(_)
                | VaultlessError::RateLimitExceeded(_)
                | VaultlessError::MessageExpired
                | VaultlessError::MessageAccessLimitReached
                | VaultlessError::NotFound(_)
                | VaultlessError::Duplicate(_)
                | VaultlessError::Conflict(_)
                | VaultlessError::Timeout(_)
                | VaultlessError::Serialization(_)
        )
    }

    pub fn status_code(&self) -> u16 {
        match self {
            VaultlessError::NotFound(_) => 404,
            VaultlessError::Unauthorized(_) => 401,
            VaultlessError::EmailNotVerified(_) => 401,
            VaultlessError::Forbidden(_) => 403,
            VaultlessError::InvalidApiKey => 401,
            VaultlessError::ApiKeyExpired => 401,
            VaultlessError::ApiKeyInactive => 403,
            VaultlessError::QuotaExceeded(_) => 429,
            VaultlessError::RateLimitExceeded(_) => 429,
            VaultlessError::IntegrityCheckFailed(_) => 403,
            VaultlessError::Validation(_)
            | VaultlessError::BadRequest(_)
            | VaultlessError::InvalidInput(_)
            | VaultlessError::MissingRequiredField(_) => 400,
            VaultlessError::Duplicate(_) | VaultlessError::Conflict(_) => 409,
            VaultlessError::MessageExpired => 410,
            VaultlessError::MessageAccessLimitReached => 410,
            VaultlessError::Timeout(_) => 408,
            VaultlessError::Serialization(_) => 500,
            _ => 500,
        }
    }
}

// -------------------------------------------------------------------------
// Implement From trait for Redis Pool Errors (UNCHANGED)
// -------------------------------------------------------------------------

/// Converts a deadpool-redis connection pool error into VaultlessError::Internal.
impl From<DeadpoolRedisPoolError> for VaultlessError {
    fn from(e: DeadpoolRedisPoolError) -> Self {
        VaultlessError::Internal(format!("Redis pool connection error: {}", e))
    }
}

/// Converts a standard redis-rs error into VaultlessError::Internal.
impl From<RedisError> for VaultlessError {
    fn from(e: RedisError) -> Self {
        VaultlessError::Internal(format!("Redis command error: {}", e))
    }
}

/// Converts a serde_json::Error. If the error is due to an IO error, it's Internal;
/// otherwise, it's treated as a BadRequest.
impl From<SerdeJsonError> for VaultlessError {
    fn from(e: SerdeJsonError) -> Self {
        if e.is_io() {
            VaultlessError::Internal(format!("Serialization IO error: {}", e))
        } else {
            // Treat user-provided invalid JSON (parsing error) as a client error (400)
            VaultlessError::BadRequest(format!("Invalid JSON format: {}", e))
        }
    }
}

impl From<pasetors::errors::Error> for VaultlessError {
    fn from(err: pasetors::errors::Error) -> Self {
        VaultlessError::Internal(format!("PASETO error: {err}"))
    }
}
