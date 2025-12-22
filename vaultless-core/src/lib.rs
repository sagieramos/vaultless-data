// =============================================================================
// VAULTLESS CORE - Library Root
// =============================================================================
//! Core library for Vaultless Data platform.
//!
//! This crate provides:
//! - Cryptographic primitives (AES-256-GCM, Ed25519)
//! - Database models and operations
//! - Session management
//! - Usage metrics and billing
//! - Error types

pub mod circuit_breaker;
pub mod crypto;
pub mod error;
pub mod models;
pub mod types;
pub mod utils;

// =============================================================================
// RE-EXPORTED CRATES
// =============================================================================
// These crates are re-exported to ensure vaultless-api uses the same versions
// and avoids duplicate dependencies in memory.

/// Re-export chrono for date/time handling
pub use chrono;

/// Re-export deadpool-redis for connection pooling
pub use deadpool_redis;

/// Re-export futures utilities
pub use futures;
pub use futures_util;

/// Re-export moka for caching
pub use moka;

/// Re-export once_cell for lazy statics
pub use once_cell;

/// Re-export prometheus for metrics
pub use prometheus;

/// Re-export redis client
pub use redis;

/// Re-export regex for pattern matching
pub use regex;

/// Re-export serde for serialization
pub use serde;
pub use serde_json;

/// Re-export sqlx for database operations
pub use sqlx;

/// Re-export thiserror for error handling
pub use thiserror;

/// Re-export tokio runtime
pub use tokio;

/// Re-export tracing for logging
pub use tracing;

/// Re-export uuid for unique identifiers
pub use uuid;

/// Re-export validator for input validation
pub use validator;

/// Re-export bigdecimal as Decimal
pub use bigdecimal::BigDecimal as Decimal;

/// Re-export getrandom for secure random number generation
pub use getrandom;

// =============================================================================
// CORE TYPES
// =============================================================================

pub use error::{Result, VaultlessError};
pub use types::SubscriptionTier;

// =============================================================================
// MODELS
// =============================================================================

pub use models::{
    // API Keys
    ApiKey,
    CreateApiKey,
    // Proofs
    CreateProof,
    MessageProof,
    ProofVerificationResult,
    VerifyProofRequest,
    // User & Sessions
    RefreshToken,
    User,
    UserSession,
    // Application & Integrity
    app_model::integrity::AttestationService,
    app_model::{
        dto::{
            Application, ApplicationKeyView, CreateApplication, CreateApplicationResponse,
            UpdateApplication,
        },
        material_view_helper::get_global_mv_etag,
    },
    // Billing
    billing::*,
    // Client Tokens
    client_token::*,
    // Clients
    clients::dto::{
        AuthenticationChallenge, Client, LoginClientRequest, LoginClientResponse,
        SignupClientRequest, SignupClientResponse,
    },
    // Messages
    message::*,
    // Sessions
    session::{
        claims_keys,
        paseto_session::{SessionData, SessionKeyManager},
    },
    // Usage & Metrics
    usage::{
        DailyUsageSummary, FlusherMetrics, MetricCounters, MetricsConfig, MonthlyTotal,
        UsageTrends, get_aggregate_by_application_id, get_realtime_usage, get_usage_trends,
        increment_rate_limit_hit_pool, start_redis_flusher,
    },
};

// =============================================================================
// CRYPTO
// =============================================================================

pub use crypto::{
    EncryptedData, SignedData, decrypt, encrypt, generate_encryption_key, generate_signing_keypair,
    hash_content, sign_data, verify_hash, verify_signature,
};

// =============================================================================
// VERSION
// =============================================================================

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
