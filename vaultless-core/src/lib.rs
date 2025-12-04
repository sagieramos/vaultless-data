pub mod crypto;
pub mod error;
pub mod models;
pub mod types;
pub mod utils;
pub mod circuit_breaker;

// Re-export commonly used types
pub use error::{Result, VaultlessError};
pub use types::SubscriptionTier;
pub use futures_util;

// Re-export models
pub use models::{
    ApiKey,
    CreateApiKey,
    CreateProof,
    MessageProof,
    ProofVerificationResult,
    RefreshToken,
    User,
    UserSession,
    VerifyProofRequest,
    app_model::attestation::AttestationService,
    app_model::{
        dto::{
            Application, ApplicationKeyView, CreateApplication,
            CreateApplicationResponse, PaginatedApplicationsWithKeys, UpdateApplication,
        },
        material_view_helper::get_global_mv_etag,
    },

    // application::{Application, CreateApplication},
    billing::*,
    client_token::*,
    clients::dto::{
        AuthenticateClientRequest, AuthenticateClientResponse, AuthenticationChallenge, Client,
        RegisterClientRequest, RegisterClientResponse,
    },

    dashboard::get_live_usage,

    message::*,
    session::{
        claims_keys,
        paseto_session::{SessionData, SessionKeyManager},
    },
    usage::{
        FlusherMetrics, MetricCounters, MetricsConfig, get_aggregate_by_application_id,
        increment_rate_limit_hit_pool, start_redis_flusher,
    },
    usage_timescale::{
        DailyUsageSummary, MonthlyTotal, UsageTrends, get_realtime_usage, get_usage_trends,
    },
};

// Re-export crypto functions
pub use crypto::{
    EncryptedData, SignedData, decrypt, encrypt, generate_encryption_key, generate_signing_keypair,
    hash_content, sign_data, verify_hash, verify_signature,
};

pub use bigdecimal::BigDecimal as Decimal;
pub use getrandom;

// Version info
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
