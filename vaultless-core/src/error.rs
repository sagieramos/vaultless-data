use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultlessError {
    // Database errors
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Duplicate record: {0}")]
    Duplicate(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    // Validation errors
    #[error("Validation error: {0}")]
    Validation(String),

    // Authentication / Authorization errors
    #[error("Unauthorized")]
    Unauthorized(String),

    #[error("Invalid API key")]
    InvalidApiKey,

    #[error("API key expired")]
    ApiKeyExpired,

    #[error("API key inactive")]
    ApiKeyInactive,

    // Business logic errors
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Message expired")]
    MessageExpired,

    #[error("Message access limit reached")]
    MessageAccessLimitReached,

    // Cryptography errors
    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption error: {0}")]
    Decryption(String),

    #[error("Signature verification failed")]
    SignatureVerificationFailed,

    #[error("Invalid proof")]
    InvalidProof,

    // Generic errors
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, VaultlessError>;

impl VaultlessError {
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            VaultlessError::Validation(_)
                | VaultlessError::Unauthorized(_)
                | VaultlessError::InvalidApiKey
                | VaultlessError::ApiKeyExpired
                | VaultlessError::ApiKeyInactive
                | VaultlessError::QuotaExceeded(_)
                | VaultlessError::RateLimitExceeded
                | VaultlessError::MessageExpired
                | VaultlessError::MessageAccessLimitReached
                | VaultlessError::NotFound(_)
                | VaultlessError::Duplicate(_)
                | VaultlessError::Conflict(_)
        )
    }

    pub fn status_code(&self) -> u16 {
        match self {
            VaultlessError::NotFound(_) => 404,
            VaultlessError::Unauthorized(_) => 401,
            VaultlessError::InvalidApiKey => 401,
            VaultlessError::ApiKeyExpired => 401,
            VaultlessError::ApiKeyInactive => 403,
            VaultlessError::QuotaExceeded(_) => 429,
            VaultlessError::RateLimitExceeded => 429,
            VaultlessError::Validation(_) => 400,
            VaultlessError::Duplicate(_) | VaultlessError::Conflict(_) => 409,
            VaultlessError::MessageExpired => 410,
            VaultlessError::MessageAccessLimitReached => 410,
            _ => 500,
        }
    }
}
