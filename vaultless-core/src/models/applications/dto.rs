use super::integrity::dto::AppMetaData;
use super::integrity::dto::IntegrityConfig;
use super::integrity::dto::PlatformConfigVersion;
use super::integrity::integrity_handler::IntegrityConfigHandler;
use crate::cache_key;
use crate::types::SubscriptionTier;
use bigdecimal::BigDecimal as Decimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use std::clone::Clone;
use std::fmt::Debug;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

/// Application table model — matches the `public.applications` schema.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Application {
    pub id: Uuid,
    pub user_id: Uuid,
    pub subscription_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub deletion_requested_at: Option<DateTime<Utc>>,
    pub internal_notes: Option<String>,
    pub app_meta: Json<AppMetaData>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplication {
    pub user_id: Uuid,

    #[validate(length(min = 1, max = 255))]
    pub name: String,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    /// Maximum TTL for messages. Defaults at DB level.
    pub max_ttl_seconds: Option<i32>,

    pub is_key_rotation_forced: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "name": "My Application",
    "description": "Updated application description",
    "isActive": true,
    "maxTtlSeconds": 3600,
    "isKeyRotationForced": false,
    "internalNotes": "Some internal notes",
    "webhooks": [
        {
            "id": null,
            "url": "https://example.com/webhooks/vaultless",
            "eventType": "client.signup",
            "isActive": true
        },
        {
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "url": "https://example.com/webhooks/delivered",
            "eventType": "client.signin",
            "isActive": true
        }
    ],
    "integrityConfig": {
        "allowUnauthenticated": false,
        "browser": {
            "authorizedOrigins": ["https://example.com", "https://app.example.com"],
            "reattestationDays": 30,
            "requireOriginHeader": true,
            "requireRefererHeader": true,
            "corsStrictMode": true,
            "requireCaptchaOnRegistration": true,
            "captchaProvider": "turnstile",
            "captchaSiteKey": "0x4AAAAAAAA...",
            "bindClientToOrigin": true,
            "trackOriginChanges": true,
            "maxOriginChangesPerClient": 3,
            "maxClientsPerIp": 50,
            "maxRegistrationsPerIpPerHour": 5,
            "maxRequestsPerIpPerHour": 300,
            "alertOnUsageSpike": true,
            "usageSpikeThreshold": 3.0,
            "usageBaselineHours": 24
        },
        "ios": {
            "reattestationDays": 30,
            "appleTeamId": "ABCD123456",
            "allowedBundleIds": ["com.example.app"],
            "allowedCertificateHashes": ["abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"],
            "minVersionCode": 100,
            "rejectUntrustedDevice": true
        },
        "android": {
            "reattestationDays": 30,
            "allowedCertificateSha256": ["abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"],
            "allowedPackageNames": ["com.example.app"],
            "minVersionCode": 100,
            "rejectUntrustedDevice": true,
            "rejectUnrecognizedVersion": true,
            "rejectUnlicensedApp": false,
            "googleCloudProject": "my-project-12345",
            "googleApiKey": "AIza...",
            "maxTokenAgeSeconds": 60
        },
        "iot": {
            "reattestationDays": 7,
            "allowedCertificateAuthorities": ["CN=MyRootCA,O=Example Inc"],
            "requireValidCertificateExpiry": true,
            "rejectFutureCertificates": true,
            "requireCnMatch": true,
            "requiredSanFields": ["DNS:device.example.com"],
            "allowedModels": ["ESP32-S3", "Raspberry-Pi-4"],
            "allowedHardwareRevisions": ["v1.2", "v2.0"],
            "allowedManufacturers": ["Espressif", "Raspberry Pi Foundation"],
            "minFirmwareVersion": 1000,
            "allowedSecureElementIds": ["SE050-001", "SE050-002"],
            "maxDeviceIdleSeconds": 86400,
            "requireChallengeSignature": true,
            "strictMode": true
        },
        "rateLimits": {
            "maxAttestationsPerUserPerHour": 50,
            "maxFailedAttemptsBeforeLockout": 5
        },
        "allowedPlatforms": {
            "browser": true,
            "ios": true,
            "android": true,
            "iot": true
        }
    }
}))]
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

    /// Webhooks configuration for this application.
    ///
    /// **Behavior:**
    /// - If `webhooks` is `None` (field omitted): No changes to webhooks
    /// - If `webhooks` is `Some([...])`: Replace all webhooks with the provided list
    ///   - Webhooks with `id: null` will be created
    ///   - Webhooks with `id: <uuid>` will be updated
    ///   - Existing webhooks not in the list will be deleted
    ///
    /// **Limits:**
    /// - Maximum 5 webhooks per application
    /// - Each (application_id, url, event_type) combination must be unique
    #[validate(custom(function = "validate_webhooks"))]
    pub webhooks: Option<Vec<WebhookInput>>,

    #[schema(value_type = Option<IntegrityConfig>)]
    pub integrity_config: Option<IntegrityConfig>,
}

/// Validates webhooks list
fn validate_webhooks(webhooks: &Vec<WebhookInput>) -> Result<(), validator::ValidationError> {
    // Check max count
    if webhooks.len() > MAX_WEBHOOKS_PER_APPLICATION {
        let mut err = validator::ValidationError::new("max_webhooks_exceeded");
        err.message = Some(std::borrow::Cow::Owned(format!(
            "Maximum {} webhooks allowed per application",
            MAX_WEBHOOKS_PER_APPLICATION
        )));
        return Err(err);
    }

    // Check for duplicate (url, event_type) combinations
    let mut seen = std::collections::HashSet::new();
    for webhook in webhooks {
        let key = (webhook.url.clone(), webhook.event_type);
        if !seen.insert(key) {
            let mut err = validator::ValidationError::new("duplicate_webhook");
            err.message = Some(std::borrow::Cow::Owned(format!(
                "Duplicate webhook: url '{}' with event_type '{}' already exists",
                webhook.url, webhook.event_type
            )));
            return Err(err);
        }
    }

    // Validate each webhook
    for webhook in webhooks {
        webhook.validate().map_err(|e| {
            let mut err = validator::ValidationError::new("invalid_webhook");
            err.message = Some(std::borrow::Cow::Owned(format!("Invalid webhook: {}", e)));
            err
        })?;
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, FromRow, Deserialize, Serialize)]
pub struct ApplicationKeyView {
    // Application Fields
    pub app_id: Uuid,
    pub app_user_id: Uuid,
    pub app_name: String,
    pub app_description: Option<String>,
    pub app_is_active: bool,
    pub app_max_ttl_seconds: i32,
    pub app_is_key_rotation_forced: bool,
    pub app_meta: serde_json::Value,

    // Secret Key Fields (Prefix info only)
    pub sk_id: Uuid,
    pub sk_key_prefix: String,

    pub sub_tier: SubscriptionTier,
    pub sub_monthly_message_quota: i64,
    pub sub_message_retention_seconds: i64,
    pub sub_rate_limit_per_minute: i32,
}

impl ApplicationKeyView {
    pub fn integrity(&self) -> crate::error::Result<IntegrityConfigHandler> {
        IntegrityConfigHandler::new_from_jsonb(&self.app_meta)
    }

    /// Generate the auth cache key for this application key view.
    ///
    /// For secret keys, uses the sk_prefix to generate the cache key.
    /// For publishable keys, returns None (caller should have the pk).
    ///
    /// Returns Some(cache_key) if this is a secret key, None if publishable.
    pub fn auth_cache_key(&self) -> Option<String> {
        // If sk_key_prefix starts with "sk_", this is a secret key
        if self.sk_key_prefix.starts_with("sk_") {
            Some(secret_key_resolution_cache_key(&self.sk_key_prefix))
        } else {
            // For publishable keys, we don't have the plaintext pk stored
            // Caller should use publishable_key_resolution_cache_key with the original pk
            None
        }
    }
}

// Usage:
/* auth_config.integrity().requires_attestation(Platform::IOS);
application.integrity().get_app_meta()?; */


#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishableKey {
    pub id: Uuid,
    pub key_prefix: String,
    pub publishable_key_plaintext: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Webhook response object returned from the API
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    pub id: Uuid,
    pub url: String,
    pub event_type: WebhookEventType,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Maximum number of webhooks allowed per application
pub const MAX_WEBHOOKS_PER_APPLICATION: usize = 5;

/// Event types that can trigger webhooks.
///
/// These events correspond to significant actions within an application
/// that developers may want to be notified about in real-time.
///
/// Note: Message events (created, delivered, read, expired) are intentionally
/// excluded as they are hotpath operations that would add unacceptable latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    // -------------------------------------------------------------------------
    // Client Events - Authentication and lifecycle
    // -------------------------------------------------------------------------
    /// Triggered when a new client registers/signs up under an application
    #[serde(rename = "client.signup")]
    ClientSignup,

    /// Triggered when an existing client signs in/authenticates
    #[serde(rename = "client.signin")]
    ClientSignin,

    /// Triggered when a client is deactivated or revoked
    #[serde(rename = "client.revoked")]
    ClientRevoked,

    /// Triggered when a client's platform attestation status changes
    #[serde(rename = "client.attestation_changed")]
    ClientAttestationChanged,

    // -------------------------------------------------------------------------
    // Security Events - Important security-related notifications
    // -------------------------------------------------------------------------
    /// Triggered when rate limiting is applied to a client
    #[serde(rename = "security.rate_limited")]
    SecurityRateLimited,

    /// Triggered when suspicious activity is detected
    #[serde(rename = "security.suspicious_activity")]
    SecuritySuspiciousActivity,

    // -------------------------------------------------------------------------
    // Quota Events - Application-level quota notifications
    // -------------------------------------------------------------------------
    /// Triggered when quota usage reaches warning threshold (e.g., 80%)
    #[serde(rename = "quota.warning")]
    QuotaWarning,

    /// Triggered when quota is exceeded
    #[serde(rename = "quota.exceeded")]
    QuotaExceeded,
}

impl WebhookEventType {
    /// Returns the string representation of the event type
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClientSignup => "client.signup",
            Self::ClientSignin => "client.signin",
            Self::ClientRevoked => "client.revoked",
            Self::ClientAttestationChanged => "client.attestation_changed",
            Self::SecurityRateLimited => "security.rate_limited",
            Self::SecuritySuspiciousActivity => "security.suspicious_activity",
            Self::QuotaWarning => "quota.warning",
            Self::QuotaExceeded => "quota.exceeded",
        }
    }

    /// Returns all available event types
    pub fn all() -> &'static [WebhookEventType] {
        &[
            Self::ClientSignup,
            Self::ClientSignin,
            Self::ClientRevoked,
            Self::ClientAttestationChanged,
            Self::SecurityRateLimited,
            Self::SecuritySuspiciousActivity,
            Self::QuotaWarning,
            Self::QuotaExceeded,
        ]
    }

    /// Returns event types grouped by category
    pub fn by_category() -> &'static [(&'static str, &'static [WebhookEventType])] {
        &[
            (
                "Client Events",
                &[
                    Self::ClientSignup,
                    Self::ClientSignin,
                    Self::ClientRevoked,
                    Self::ClientAttestationChanged,
                ],
            ),
            (
                "Security Events",
                &[Self::SecurityRateLimited, Self::SecuritySuspiciousActivity],
            ),
            ("Quota Events", &[Self::QuotaWarning, Self::QuotaExceeded]),
        ]
    }
}

impl std::fmt::Display for WebhookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for WebhookEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "client.signup" => Ok(Self::ClientSignup),
            "client.signin" => Ok(Self::ClientSignin),
            "client.revoked" => Ok(Self::ClientRevoked),
            "client.attestation_changed" => Ok(Self::ClientAttestationChanged),
            "security.rate_limited" => Ok(Self::SecurityRateLimited),
            "security.suspicious_activity" => Ok(Self::SecuritySuspiciousActivity),
            "quota.warning" => Ok(Self::QuotaWarning),
            "quota.exceeded" => Ok(Self::QuotaExceeded),
            _ => Err(format!(
                "Unknown webhook event type: '{}'. Valid types are: {}",
                s,
                Self::all()
                    .iter()
                    .map(|e| e.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// Input for creating or updating a webhook within an application update.
///
/// - If `id` is `None`, a new webhook will be created.
/// - If `id` is `Some(uuid)`, the existing webhook will be updated.
/// - Webhooks not included in the update list will be deleted.
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "id": null,
    "url": "https://example.com/webhooks/vaultless",
    "eventType": "client.signup",
    "isActive": true
}))]
pub struct WebhookInput {
    /// Webhook ID. If provided, updates existing webhook. If null/omitted, creates new webhook.
    #[schema(value_type = Option<String>)]
    pub id: Option<Uuid>,

    /// Webhook endpoint URL (must be HTTPS in production)
    #[validate(url, length(max = 2048))]
    pub url: String,

    /// Event type that triggers this webhook
    pub event_type: WebhookEventType,

    /// Whether this webhook is active
    #[serde(default = "default_webhook_active")]
    pub is_active: bool,
}

fn default_webhook_active() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationWithUsage {
    pub application_id: Uuid,
    #[serde(skip_serializing)]
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[schema(value_type = AppMetaData)]
    pub app_meta: Json<AppMetaData>,

    // Subscription Pool (Shared)
    #[serde(skip_serializing)]
    pub subscription_id: Option<Uuid>,
    pub tier: Option<String>,
    pub monthly_message_quota: i64,
    pub rate_limit_per_minute: i32,
    pub message_retention_seconds: i64,

    // Keys
    #[serde(skip_serializing)]
    pub secret_key_id: Option<Uuid>,
    pub secret_key_prefix: Option<String>,
    pub publishable_key_count: i64,
    #[schema(value_type = Vec<PublishableKey>)]
    pub publishable_keys: Json<Vec<PublishableKey>>,

    // Webhooks & Clients
    pub webhook_count: i64,
    #[schema(value_type = Vec<Webhook>)]
    pub webhooks: Json<Vec<Webhook>>,
    pub client_count: i64,

    // Monthly Metrics
    pub current_month_messages_sent: i64,
    pub current_month_messages_received: i64,
    pub current_month_proofs_verified: i64,
    pub current_month_bytes_stored: i64,
    pub current_month_bytes_sent: i64,
    pub current_month_bytes_received: i64,
    pub current_month_rate_limit_hits: i64,
    pub current_month_cost_cents: i64,
    #[schema(value_type = f64)]
    pub quota_usage_percentage: Decimal,

    // Totals
    pub lifetime_messages_sent: i64,
    pub lifetime_cost_cents: i64,
}


/// Minimal cache-only struct for Redis HASH storage.
/// Contains only fields needed for hot-path authorization and rate-limiting.
/// Stored as Redis HASH for O(1) field access and Lua script compatibility.
#[derive(Debug, Clone)]
pub struct AuthCacheEntry {
    pub app_id: Uuid,
    pub user_id: Uuid,
    pub is_active: bool,
    pub rotation_forced: bool,
    pub sk_id: Uuid,
    pub sk_prefix: String,
    pub tier: String,
    pub rate_limit_per_minute: i32,
    pub monthly_quota: i64,
    pub retention_seconds: i64,
}

/// Redis field names for AuthCacheEntry HASH storage
pub mod auth_cache_field {
    pub const APP_ID: &str = "app_id";
    pub const USER_ID: &str = "user_id";
    pub const IS_ACTIVE: &str = "is_active";
    pub const ROTATION_FORCED: &str = "rotation_forced";
    pub const SK_ID: &str = "sk_id";
    pub const SK_PREFIX: &str = "sk_prefix";
    pub const TIER: &str = "tier";
    pub const RATE_LIMIT: &str = "rate_limit";
    pub const QUOTA: &str = "quota";
    pub const RETENTION: &str = "retention";
}

impl AuthCacheEntry {
    /// Cache TTL (1 hour)
    pub const TTL_SECONDS: i64 = 3600;

    /// Convert from Redis HASH (HashMap<String, String>)
    /// Returns None if required fields are missing
    pub fn from_redis(vals: std::collections::HashMap<String, String>) -> Option<Self> {
        Some(Self {
            app_id: vals.get(auth_cache_field::APP_ID)?.parse().ok()?,
            user_id: vals.get(auth_cache_field::USER_ID)?.parse().ok()?,
            is_active: vals.get(auth_cache_field::IS_ACTIVE).map(|v| v == "1").unwrap_or(false),
            rotation_forced: vals.get(auth_cache_field::ROTATION_FORCED).map(|v| v == "1").unwrap_or(false),
            sk_id: vals.get(auth_cache_field::SK_ID)?.parse().ok()?,
            sk_prefix: vals.get(auth_cache_field::SK_PREFIX)?.clone(),
            tier: vals.get(auth_cache_field::TIER)?.clone(),
            rate_limit_per_minute: vals.get(auth_cache_field::RATE_LIMIT)?.parse().ok()?,
            monthly_quota: vals.get(auth_cache_field::QUOTA)?.parse().ok()?,
            retention_seconds: vals.get(auth_cache_field::RETENTION)?.parse().ok()?,
        })
    }

    /// Convert to Redis HASH compatible values for hset_multiple
    /// Returns Vec of String for redis pipe
    pub fn to_redis_args(&self) -> Vec<String> {
        let mut args = Vec::with_capacity(20);
        args.push(auth_cache_field::APP_ID.to_string());
        args.push(self.app_id.to_string());
        args.push(auth_cache_field::USER_ID.to_string());
        args.push(self.user_id.to_string());
        args.push(auth_cache_field::IS_ACTIVE.to_string());
        args.push(if self.is_active { "1".to_string() } else { "0".to_string() });
        args.push(auth_cache_field::ROTATION_FORCED.to_string());
        args.push(if self.rotation_forced { "1".to_string() } else { "0".to_string() });
        args.push(auth_cache_field::SK_ID.to_string());
        args.push(self.sk_id.to_string());
        args.push(auth_cache_field::SK_PREFIX.to_string());
        args.push(self.sk_prefix.clone());
        args.push(auth_cache_field::TIER.to_string());
        args.push(self.tier.clone());
        args.push(auth_cache_field::RATE_LIMIT.to_string());
        args.push(self.rate_limit_per_minute.to_string());
        args.push(auth_cache_field::QUOTA.to_string());
        args.push(self.monthly_quota.to_string());
        args.push(auth_cache_field::RETENTION.to_string());
        args.push(self.retention_seconds.to_string());
        args
    }

    /// Convert from ApplicationKeyView (Postgres result)
    pub fn from_application_key_view(view: &ApplicationKeyView) -> Self {
        Self {
            app_id: view.app_id,
            user_id: view.app_user_id,
            is_active: view.app_is_active,
            rotation_forced: view.app_is_key_rotation_forced,
            sk_id: view.sk_id,
            sk_prefix: view.sk_key_prefix.clone(),
            tier: view.sub_tier.to_string(),
            rate_limit_per_minute: view.sub_rate_limit_per_minute,
            monthly_quota: view.sub_monthly_message_quota,
            retention_seconds: view.sub_message_retention_seconds,
        }
    }

}

impl From<ApplicationKeyView> for AuthCacheEntry {
    fn from(view: ApplicationKeyView) -> Self {
        Self::from_application_key_view(&view)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedApplicationKeyView {
    pub app_id: Uuid,
    pub app_user_id: Uuid,
    pub app_name: String,
    pub app_is_active: bool,
    pub app_max_ttl_seconds: i32,
    pub app_is_key_rotation_forced: bool,

    pub sk_id: Uuid,

    pub sub_tier: SubscriptionTier,
    pub sub_monthly_message_quota: i64,
    pub sub_message_retention_seconds: i64,
    pub sub_rate_limit_per_minute: i32,

    // Integrity/Security Metadata
    pub platform_fingerprint: PlatformConfigVersion,
}

impl From<ApplicationKeyView> for CachedApplicationKeyView {
    fn from(a: ApplicationKeyView) -> Self {
        let platform_fingerprint = IntegrityConfigHandler::new_from_jsonb(&a.app_meta)
            .map(|handler| handler.platform_config_version)
            .unwrap_or(PlatformConfigVersion::new());

        CachedApplicationKeyView {
            app_id: a.app_id,
            app_user_id: a.app_user_id,
            app_name: a.app_name,
            app_is_active: a.app_is_active,
            app_max_ttl_seconds: a.app_max_ttl_seconds,
            app_is_key_rotation_forced: a.app_is_key_rotation_forced,

            sk_id: a.sk_id,

            sub_tier: a.sub_tier,
            sub_monthly_message_quota: a.sub_monthly_message_quota,
            sub_message_retention_seconds: a.sub_message_retention_seconds,
            sub_rate_limit_per_minute: a.sub_rate_limit_per_minute,

            platform_fingerprint,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWarning {
    pub application_id: Uuid,
    pub application_name: String,
    #[schema(value_type = f64)]
    pub quota_usage_percentage: Decimal,
    // Note: detailed fields like remaining_quota are now calculated
    // in the frontend or specialized detailed views to keep this summary fast.
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserUsageSummary {
    /// Total number of applications owned by the user
    pub total_apps: i32,
    /// Total aggregated messages sent across all apps in the current billing cycle
    pub total_monthly_messages: i64,
    /// Total registered clients across the entire developer bundle
    pub total_clients: i64,
    /// Total estimated cost in cents for the current month
    pub total_monthly_cost: i64,
    /// Number of applications that have exceeded 90% of their shared quota
    pub critical_quota_apps: i32,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedApplicationsSummary {
    pub data: Vec<ApplicationSummary>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSummary {
    pub application_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tier: Option<String>,
    pub monthly_message_quota: Option<i64>,
    pub publishable_key_count: i64,
    pub webhook_count: i64,
    pub client_count: i64,
    // Using Decimal to maintain precision for billing calculations
    #[schema(value_type = f64)]
    pub quota_usage_percentage: Decimal,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedQuotaWarnings {
    pub data: Vec<QuotaWarning>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

pub fn secret_key_resolution_cache_key(key_hash: &str) -> String {
    cache_key!("appconfig", "sk", key_hash)
}

pub fn publishable_key_resolution_cache_key(pk_plaintext: &str) -> String {
    cache_key!("appconfig", "pk", pk_plaintext)
}

/// Real-time quota status for an application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    pub limit: i64,
    pub used: i64,
    pub remaining: i64,
    pub percentage_used: f64,
    pub is_exceeded: bool,
}

// =============================================================================
// Key Rotation DTOs
// =============================================================================

/// Response returned when rotating a secret key
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RotateSecretKeyResponse {
    /// The application ID
    pub application_id: Uuid,
    /// The new secret key (only shown once, store securely!)
    pub new_secret_key: String,
    /// Prefix of the new key for identification
    pub key_prefix: String,
    /// When the new key was created
    pub created_at: DateTime<Utc>,
    /// ID of the old key that was deactivated (for audit purposes)
    pub old_key_id: Uuid,
}

/// Response returned when rotating a publishable key
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RotatePublishableKeyResponse {
    /// The application ID
    pub application_id: Uuid,
    /// The new publishable key
    pub new_publishable_key: String,
    /// Prefix of the new key for identification
    pub key_prefix: String,
    /// When the new key was created
    pub created_at: DateTime<Utc>,
    /// ID of the old key that was deactivated (for audit purposes)
    pub old_key_id: Uuid,
}

/// Response returned when adding an additional publishable key
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AddPublishableKeyResponse {
    /// The application ID
    pub application_id: Uuid,
    /// The new publishable key
    pub new_publishable_key: String,
    /// Prefix of the new key for identification
    pub key_prefix: String,
    /// When the new key was created
    pub created_at: DateTime<Utc>,
    /// Total number of active publishable keys for this application
    pub total_active_publishable_keys: i64,
}
