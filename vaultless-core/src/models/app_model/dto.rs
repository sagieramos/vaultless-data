use crate::cache_key;
use crate::models::{ApiKey, CachedApiKey};
use crate::types::{KeyType, SubscriptionTier};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

/// A comprehensive report on the validity and status of an incoming API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationValidation {
    /// Overall result: True if all critical checks passed.
    pub is_valid: bool,

    // --- Application Status ---
    /// Is the application marked as active by the owner?
    pub application_active: bool,

    // --- API Key Status ---
    /// Is this specific key marked as active?
    pub api_key_active: bool,
    /// Has the key passed its explicit expiration date?
    pub api_key_expired: bool,
    /// The key's planned expiration time (used for checks).
    pub api_key_expires_at: Option<DateTime<Utc>>,

    // --- Subscription & Quota ---
    /// The service tier governing limits.
    pub tier: SubscriptionTier,
    /// The current status of the monthly quota.
    pub quota_status: QuotaStatus,

    // --- Metrics ---
    /// The current monthly usage count.
    pub monthly_usage_count: i64,
    /// The maximum monthly quota allowed by the tier.
    pub monthly_quota_limit: i64,

    // --- Errors ---
    /// A collection of all validation errors found.
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuotaStatus {
    /// Usage is within the limit.
    Ok,
    /// Usage is approaching the limit (e.g., > 80%).
    Warning,
    /// Usage has exceeded the monthly limit.
    Exhausted,
}

/// Defines the severity and type of validation failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationFailureType {
    /// The API key or application is explicitly disabled.
    Deactivated,
    /// The API key has passed its expiration date.
    Expired,
    /// The monthly quota has been exceeded.
    QuotaExhausted,
    /// The rate limit for the current minute/period has been hit.
    RateLimitHit,
    ErrorRedis,
    NotFound,
    Internal,
    Forbidden,
}

/// Represents a specific validation failure encountered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// The specific type of failure.
    pub type_code: ValidationFailureType,
    /// A human-readable message explaining the failure.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Critical, // Blocks operation
    Warning,  // Operation continues but user should be notified
    Info,     // FYI only
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationHealth {
    pub status: HealthStatus,
    pub application_id: Uuid,
    pub is_active: bool,
    pub tier: Option<SubscriptionTier>,
    pub quota: Option<QuotaStatus>,
    pub issues: Vec<ValidationError>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,   // All good
    Warning,   // Working but has warnings (e.g., near quota)
    Unhealthy, // Critical issues (inactive, expired, quota exceeded)
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Application {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub secret_key_id: Uuid,
    pub authorized_origin: Option<String>,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateApplication {
    pub user_id: Uuid,

    #[validate(length(min = 1, max = 255))]
    pub name: String,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    pub tier: SubscriptionTier,

    pub existing_api_key_id: Option<Uuid>,

    #[validate(length(max = 255))]
    pub bundle_id: Option<String>,

    #[validate(length(max = 50))]
    pub platform: Option<String>,

    #[validate(url, length(max = 255))]
    pub webhook_url: Option<String>,
}

/// Application with denormalized tier information from api_keys and the Publishable Key from a separate JOIN.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApplicationWithTier {
    // All Application fields (modified)
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub secret_key_id: Uuid,
    pub authorized_origin: Option<String>, // <-- added
    // REMOVED: pub publishable_key: String,
    // REMOVED: pub publishable_key_prefix: String,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Data from the Secret Key's api_keys record
    pub tier: SubscriptionTier,
    pub monthly_message_quota: i32,
    pub rate_limit_per_minute: i32,
    pub message_retention_seconds: i32,
    pub api_key_active: bool,

    // Data from the Publishable Key's api_keys record (requires a second JOIN)
    // You must select this column as 'publishable_key_plaintext' in your SQL query
    pub publishable_key_plaintext: String,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateApplication {
    #[validate(length(min = 1, max = 255))]
    pub name: Option<String>,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    #[validate(length(max = 255))]
    pub bundle_id: Option<String>,

    #[validate(length(max = 50))]
    pub platform: Option<String>,

    #[validate(url, length(max = 255))]
    pub webhook_url: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateApplicationResponse {
    pub application: Application,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>, // Only returned once at creation
    // Renamed from publishable_key to reflect the source column name
    pub publishable_key_plaintext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedKeyBundle {
    /// The associated application data.
    pub application: Application,
    /// The API Key record for the Secret Key (used for quotas/billing).
    pub secret_key_row: ApiKey,
}

// --- New Structs for Lean Caching ---

/// A lean projection of the Application struct, omitting large fields like name and description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedApplication {
    pub id: Uuid,
    pub user_id: Uuid,
    pub secret_key_id: Uuid,
    pub authorized_origin: Option<String>, // <-- NEW
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&Application> for CachedApplication {
    fn from(app: &Application) -> Self {
        Self {
            id: app.id,
            user_id: app.user_id,
            secret_key_id: app.secret_key_id,
            authorized_origin: app.authorized_origin.clone(), // <-- NEW
            bundle_id: app.bundle_id.clone(),
            platform: app.platform.clone(),
            webhook_url: app.webhook_url.clone(),
            is_active: app.is_active,
            created_at: app.created_at,
            updated_at: app.updated_at,
        }
    }
}

/// The cached version of the key bundle, using the lean Application struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResolvedKeyBundle {
    pub application: CachedApplication,
    // Use the lean ApiKey struct
    pub secret_key_row: CachedApiKey,
}

impl From<&ResolvedKeyBundle> for CachedResolvedKeyBundle {
    fn from(bundle: &ResolvedKeyBundle) -> Self {
        Self {
            application: CachedApplication::from(&bundle.application),
            secret_key_row: CachedApiKey::from(&bundle.secret_key_row),
        }
    }
}

pub enum KeyGranularity {
    Secret,
    Publishable,
}


pub fn cache_key_by_id(id: Uuid) -> String {
    cache_key!("app", "id", id)
}

// NOTE: This function needs to be updated to use the full plaintext key for caching
// if the cache key relies on the publishable key.
pub fn cache_key_by_publishable_key(pk: &str) -> String {
    cache_key!("app", "pk", pk)
}

/// Generates the Redis key for resolving a Secret Key hash to its ID.
pub fn secret_key_resolution_cache_key(key_hash: &str) -> String {
    // Example: "res:sk:2c78a9c8f..." -> Uuid
    cache_key!("res", "sk", key_hash)
}

/// Generates the Redis key for resolving a Publishable Key plaintext to its Secret Key ID.
pub fn publishable_key_resolution_cache_key(pk_plaintext: &str) -> String {
    // Example: "res:pk:pk_live_ab12..." -> Uuid
    cache_key!("res", "pk", pk_plaintext)
}
