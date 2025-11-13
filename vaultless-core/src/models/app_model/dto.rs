use crate::cache_key;
use crate::models::{ApiKey, CachedApiKey};
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
    pub secret_key_id: Uuid,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub last_successful_attestation_at: Option<DateTime<Utc>>,
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

    pub tier: SubscriptionTier,

    pub existing_api_key_id: Option<Uuid>,

    #[validate(length(min = 1, max = 255))]
    pub bundle_id: Option<String>,

    #[validate(length(max = 50))]
    pub platform: Option<String>,

    #[validate(url, length(max = 255))]
    pub webhook_url: Option<String>,

    /// The maximum TTL for messages created by this application.
    pub max_ttl_seconds: Option<i32>,

    /// Flag to force key rotation (default should be false in DB).
    pub is_key_rotation_forced: Option<bool>,

    /// JSONB configuration for integrity checks (e.g., cert hashes, authorized origins).
    pub integrity_config: Option<serde_json::Value>,
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

    /// Allows updating the maximum TTL for messages created by this application.
    pub max_ttl_seconds: Option<i32>,

    /// Allows updating the flag to force key rotation.
    pub is_key_rotation_forced: Option<bool>,

    /// Allows administrators/support staff to update internal notes.
    #[validate(length(max = 1000))]
    pub internal_notes: Option<String>,

    /// Allows updating the JSONB configuration for integrity checks (e.g., cert hashes, authorized origins).
    pub integrity_config: Option<serde_json::Value>,
    // Note: last_successful_attestation_at and deletion_requested_at are audit fields
    // and should generally not be set by a public update request.
}

/// Data Transfer Object used to display a complete view of an Application,
/// including its current tier limits and both secret and publishable key information.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApplicationWithTier {
    // Application core fields
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub secret_key_id: Uuid,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // NEW SECURITY & CONFIG FIELDS
    /// Maximum time-to-live (in seconds) allowed for newly generated messages.
    pub max_ttl_seconds: i32,
    /// If true, the system will force secret key rotation/renewal after it expires.
    pub is_key_rotation_forced: bool,
    /// Private notes visible only to internal team members (e.g., billing, support).
    pub internal_notes: Option<String>,
    /// JSONB configuration for platform integrity checks (e.g., authorized origins for 'web').
    pub integrity_config: Option<serde_json::Value>,

    // Tier/Quota data from Secret Key (ak_secret)
    pub tier: String,
    pub monthly_message_quota: i32,
    pub rate_limit_per_minute: i32,
    pub message_retention_seconds: Option<i32>,
    pub api_key_active: bool, // is_active AS api_key_active

    // Publishable Key data (ak_publishable)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedApplication {
    pub id: Uuid,
    pub user_id: Uuid,
    pub secret_key_id: Uuid,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub integrity_config: serde_json::Value,
}

impl From<&Application> for CachedApplication {
    fn from(app: &Application) -> Self {
        Self {
            id: app.id,
            user_id: app.user_id,
            secret_key_id: app.secret_key_id,
            bundle_id: app.bundle_id.clone(),
            platform: app.platform.clone(),
            webhook_url: app.webhook_url.clone(),
            is_active: app.is_active,
            created_at: app.created_at,
            updated_at: app.updated_at,
            max_ttl_seconds: app.max_ttl_seconds,
            is_key_rotation_forced: app.is_key_rotation_forced,
            integrity_config: app.integrity_config.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResolvedKeyBundle {
    pub application: CachedApplication,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedKeyBundle {
    pub application: Application,
    pub secret_key_row: ApiKey,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyGranularity {
    Publishable,
    Secret,
}

// --- Cache key helpers ---

pub fn cache_key_by_id(id: Uuid) -> String {
    cache_key!("app", "id", id)
}

pub fn cache_key_by_publishable_key(pk: &str) -> String {
    cache_key!("app", "pk", pk)
}

pub fn secret_key_resolution_cache_key(key_hash: &str) -> String {
    cache_key!("res", "sk", key_hash)
}

pub fn publishable_key_resolution_cache_key(pk_plaintext: &str) -> String {
    cache_key!("res", "pk", pk_plaintext)
}
