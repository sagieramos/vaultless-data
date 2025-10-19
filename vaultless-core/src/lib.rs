pub mod crypto;
pub mod error;
pub mod models;
pub mod types;

// Re-export commonly used types
pub use error::{Result, VaultlessError};
pub use types::SubscriptionTier;

// Re-export models
pub use models::{
    ApiKey, CreateApiKey, CreateMessage, CreateProof, DailyUsageSummary, Message, MessageMetadata,
    MessageProof, MonthlyTotal, ProofVerificationResult, RefreshToken, UsageMetric, UsageSummary,
    UsageTrends, User, UserSession, VerifyProofRequest, WeeklyUsageSummary, notification::*,
};

// Re-export crypto functions
pub use crypto::{
    EncryptedData, SignedData, decrypt, encrypt, generate_encryption_key, generate_signing_keypair,
    hash_content, sign_data, verify_hash, verify_signature,
};

pub use getrandom;

// Version info
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
