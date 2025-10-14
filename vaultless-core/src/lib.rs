pub mod error;
pub mod models;
pub mod types;

// Re-export commonly used types
pub use error::{Result, VaultlessError};
pub use types::SubscriptionTier;

// Re-export models
pub use models::{
    ApiKey, CreateApiKey, CreateMessage, CreateProof, Message, MessageMetadata, MessageProof,
    ProofVerificationResult, UsageMetric, UsageSummary, VerifyProofRequest,
};

// Version info
pub const VERSION: &str = env!("CARGO_PKG_VERSION");