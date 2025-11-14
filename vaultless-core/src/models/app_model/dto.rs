use crate::cache_key;
use crate::types::{KeyType, SubscriptionTier};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

/// Application table model — matches the `public.applications` schema.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Application {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub deletion_requested_at: Option<DateTime<Utc>>,
    pub internal_notes: Option<String>,
    pub integrity_config: serde_json::Value,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateApplication {
    pub user_id: Uuid,

    #[validate(length(min = 1, max = 255))]
    pub name: String,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    /// Maximum TTL for messages. Defaults at DB level.
    pub max_ttl_seconds: Option<i32>,

    pub is_key_rotation_forced: Option<bool>,

    pub integrity_config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateApplication {
    #[validate(length(min = 1, max = 255))]
    pub name: Option<String>,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    pub is_active: Option<bool>,

    pub max_ttl_seconds: Option<i32>,

    pub is_key_rotation_forced: Option<bool>,

    #[validate(length(max = 1000))]
    pub internal_notes: Option<String>,

    pub integrity_config: Option<serde_json::Value>,
}

/// Data Transfer Object used to display a complete view of an Application,
/// including its current tier limits and both secret and publishable key information.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApplicationWithTier {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub internal_notes: Option<String>,
    pub integrity_config: Option<serde_json::Value>,

    // Joined secret key values
    pub tier: String,
    pub monthly_message_quota: i32,
    pub rate_limit_per_minute: i32,
    pub message_retention_seconds: Option<i32>,
    pub api_key_active: bool,

    // Publishable key
    pub publishable_key_plaintext: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateApplicationResponse {
    pub application: Application,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,
    pub publishable_key_plaintext: String,
}

// --- Caching models for Redis ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyGranularity {
    Publishable,
    Secret,
}
/// Represents the structured content of the JSONB 'integrity_config' column.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrityConfig {
    #[serde(default)]
    pub web: WebIntegrityConfig,
    #[serde(default)]
    pub ios: MobileIntegrityConfig,
    #[serde(default)]
    pub android: MobileIntegrityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct WebIntegrityConfig {
    /// A list of allowed origins (e.g., ["https://my.app.com"]). Max 5.
    #[validate(length(max = 5))]
    pub authorized_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Validate)]
pub struct MobileIntegrityConfig {
    /// REQUIRED: The expected SHA256 hash of the application's signing certificate.
    #[validate(custom(function = "validate_sha256_format"))]
    #[serde(default)]
    pub allowed_certificate_sha256: Option<String>,

    /// OPTIONAL: A list of accepted package/bundle identifiers. Max 5.
    #[validate(length(max = 5))]
    pub allowed_bundle_ids: Vec<String>,

    /// OPTIONAL: Minimum acceptable version of the application.
    #[serde(default)]
    pub min_version_code: Option<i32>,

    /// OPTIONAL: Flag to reject untrusted devices.
    #[serde(default)]
    pub reject_untrusted_device: bool,
}

/// DTO for updating the entire configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateIntegrityConfigRequest {
    pub web: WebIntegrityConfig,
    pub ios: MobileIntegrityConfig,
    pub android: MobileIntegrityConfig,
}

// --- CORRECTED VALIDATION FUNCTION ---

/// Custom validator for an optional SHA256 string.
/// Accepts `&Option<String>` and validates only if Some.
fn validate_sha256_format(h: &str) -> std::result::Result<(), validator::ValidationError> {
    if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(validator::ValidationError::new("invalid_sha256"));
    }
    Ok(())
}

#[derive(Debug, Clone, FromRow, Deserialize)]
pub struct AuthConfig {
    pub app_id: Uuid,
    pub app_user_id: Uuid,
    pub app_name: String,
    pub app_description: Option<String>,
    pub app_is_active: bool,
    pub app_max_ttl_seconds: i32,
    pub app_is_key_rotation_forced: bool,
    pub app_integrity_config: serde_json::Value,

    pub sk_id: Uuid,
    pub sk_key_prefix: String,
    pub sk_tier: Option<SubscriptionTier>,
    pub sk_monthly_message_quota: Option<i32>,
    pub sk_message_retention_seconds: Option<i32>,
    pub sk_rate_limit_per_minute: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedAuthConfig {
    pub app_id: Uuid,
    pub app_user_id: Uuid,
    pub app_name: String,
    pub app_is_active: bool,
    pub app_max_ttl_seconds: i32,
    pub app_is_key_rotation_forced: bool,
    pub app_integrity_config: serde_json::Value,

    pub sk_id: Uuid,
    pub sk_tier: Option<crate::types::SubscriptionTier>,
    pub sk_monthly_message_quota: Option<i32>,
    pub sk_message_retention_seconds: Option<i32>,
    pub sk_rate_limit_per_minute: Option<i32>,
}

impl From<AuthConfig> for CachedAuthConfig {
    fn from(a: AuthConfig) -> Self {
        CachedAuthConfig {
            app_id: a.app_id,
            app_user_id: a.app_user_id,
            app_name: a.app_name,
            app_is_active: a.app_is_active,
            app_max_ttl_seconds: a.app_max_ttl_seconds,
            app_is_key_rotation_forced: a.app_is_key_rotation_forced,
            app_integrity_config: a.app_integrity_config,

            sk_id: a.sk_id,
            sk_tier: a.sk_tier,
            sk_monthly_message_quota: a.sk_monthly_message_quota,
            sk_message_retention_seconds: a.sk_message_retention_seconds,
            sk_rate_limit_per_minute: a.sk_rate_limit_per_minute,
        }
    }
}

// --- Cache key helpers ---

pub fn secret_key_resolution_cache_key(key_hash: &str) -> String {
    cache_key!("res", "sk", key_hash)
}

pub fn publishable_key_resolution_cache_key(pk_plaintext: &str) -> String {
    cache_key!("res", "pk", pk_plaintext)
}
