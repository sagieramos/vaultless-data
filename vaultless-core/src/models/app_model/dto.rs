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
#[schema(example = json!({
    "name": "My Application",
    "description": "Updated application description",
    "is_active": true,
    "max_ttl_seconds": 3600,
    "is_key_rotation_forced": false,
    "internal_notes": "Some internal notes",
    "integrity_config": {
        "allow_unauthenticated": false,
        "browser": {
            "authorized_origins": ["https://example.com", "https://app.example.com"],
            "reattestation_days": 30,
            "require_origin_header": true,
            "require_referer_header": true,
            "cors_strict_mode": true,
            "require_captcha_on_registration": true,
            "captcha_provider": "turnstile",
            "captcha_site_key": "0x4AAAAAAAA...",
            "bind_client_to_origin": true,
            "track_origin_changes": true,
            "max_origin_changes_per_client": 3,
            "max_clients_per_ip": 50,
            "max_registrations_per_ip_per_hour": 5,
            "max_requests_per_ip_per_hour": 300,
            "alert_on_usage_spike": true,
            "usage_spike_threshold": 3.0,
            "usage_baseline_hours": 24
        },
        "ios": {
            "reattestation_days": 30,
            "apple_team_id": "ABCD123456",
            "allowed_bundle_ids": ["com.example.app"],
            "allowed_certificate_hashes": ["abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"],
            "min_version_code": 100,
            "reject_untrusted_device": true
        },
        "android": {
            "reattestation_days": 30,
            "allowed_certificate_sha256": ["abcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234"],
            "allowed_package_names": ["com.example.app"],
            "min_version_code": 100,
            "reject_untrusted_device": true,
            "reject_unrecognized_version": true,
            "reject_unlicensed_app": false,
            "google_cloud_project": "my-project-12345",
            "google_api_key": "AIza...",
            "max_token_age_seconds": 60
        },
        "iot": {
            "reattestation_days": 7,
            "allowed_certificate_authorities": ["CN=MyRootCA,O=Example Inc"],
            "require_valid_certificate_expiry": true,
            "reject_future_certificates": true,
            "require_cn_match": true,
            "required_san_fields": ["DNS:device.example.com"],
            "allowed_models": ["ESP32-S3", "Raspberry-Pi-4"],
            "allowed_hardware_revisions": ["v1.2", "v2.0"],
            "allowed_manufacturers": ["Espressif", "Raspberry Pi Foundation"],
            "min_firmware_version": 1000,
            "allowed_secure_element_ids": ["SE050-001", "SE050-002"],
            "max_device_idle_seconds": 86400,
            "require_challenge_signature": true,
            "strict_mode": true
        },
        "rate_limits": {
            "max_attestations_per_user_per_hour": 50,
            "max_failed_attempts_before_lockout": 5
        },
        "allowed_platforms": {
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

    #[schema(value_type = Option<IntegrityConfig>)]
    pub integrity_config: Option<IntegrityConfig>,
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

#[derive(Debug, Clone, FromRow, Deserialize)]
pub struct ApplicationKeyView {
    pub app_id: Uuid,
    pub app_user_id: Uuid,
    pub app_name: String,
    pub app_description: Option<String>,
    pub app_is_active: bool,
    pub app_max_ttl_seconds: i32,
    pub app_is_key_rotation_forced: bool,
    pub app_app_meta: serde_json::Value,

    pub sk_id: Uuid,
    pub sk_key_prefix: String,
    pub sk_tier: Option<SubscriptionTier>,
    pub sk_monthly_message_quota: Option<i32>,
    pub sk_message_retention_seconds: Option<i32>,
    pub sk_rate_limit_per_minute: Option<i32>,
}

impl ApplicationKeyView {
    pub fn integrity(&self) -> crate::error::Result<IntegrityConfigHandler> {
        IntegrityConfigHandler::new_from_jsonb(&self.app_app_meta)
    }
}

// Usage:
/* auth_config.integrity().requires_attestation(Platform::IOS);
application.integrity().get_app_meta()?; */

#[derive(Debug, Clone, FromRow)]
pub struct ApplicationWithKeysFromView {
    pub application_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub deletion_requested_at: Option<DateTime<Utc>>,
    pub app_meta: Json<AppMetaData>,

    // Publishable keys
    pub publishable_key_count: i64,
    pub publishable_keys: Json<Vec<PublishableKey>>,

    // Webhooks
    pub webhook_count: i64,
    pub webhooks: Json<Vec<Webhook>>,

    pub total_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplicationWithKeysResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub deletion_requested_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_notes: Option<String>,
    pub app_meta: serde_json::Value,

    // Publishable keys
    pub publishable_key_count: i64,
    pub publishable_keys: Json<Vec<PublishableKey>>,

    // Webhooks
    pub webhook_count: i64,
    pub webhooks: Json<Vec<Webhook>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Webhook {
    pub id: Uuid,
    pub url: String,
    pub event_type: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApplicationWithUsage {
    // Application metadata
    pub application_id: Uuid,
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
    pub app_meta: Json<AppMetaData>,

    // Secret key info
    pub secret_key_id: Option<Uuid>,
    pub tier: Option<String>,
    pub monthly_message_quota: Option<i64>,
    pub rate_limit_per_minute: Option<i32>,
    pub message_retention_seconds: Option<i32>,

    // Publishable keys
    pub publishable_key_count: i64,
    pub publishable_keys: Json<Vec<PublishableKey>>,

    // Webhooks
    pub webhook_count: i64,
    pub webhooks: Json<Vec<Webhook>>,

    // Current month usage
    pub current_month_messages_sent: i64,
    pub current_month_messages_received: i64,
    pub current_month_proofs_verified: i64,
    pub current_month_bytes_stored: i64,
    pub current_month_bytes_sent: i64,
    pub current_month_bytes_received: i64,
    pub current_month_rate_limit_hits: i64,
    pub current_month_cost_cents: i64,
    pub quota_usage_percentage: f64,

    // Lifetime usage
    pub lifetime_messages_sent: i64,
    pub lifetime_messages_received: i64,
    pub lifetime_proofs_verified: i64,
    pub lifetime_bytes_stored: i64,
    pub lifetime_bytes_sent: i64,
    pub lifetime_bytes_received: i64,
    pub lifetime_rate_limit_hits: i64,
    pub lifetime_cost_cents: i64,

    // Trend usage (7d)
    pub last_7d_messages_sent: i64,
    pub last_7d_bytes_sent: i64,
    pub last_7d_bytes_received: i64,
    pub last_7d_cost_cents: i64,

    // Trend usage (30d)
    pub last_30d_messages_sent: i64,
    pub last_30d_bytes_sent: i64,
    pub last_30d_bytes_received: i64,
    pub last_30d_cost_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaginatedApplicationsWithKeys {
    pub data: Vec<ApplicationWithKeysResponse>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
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
    pub sk_tier: Option<crate::types::SubscriptionTier>,
    pub sk_monthly_message_quota: Option<i32>,
    pub sk_message_retention_seconds: Option<i32>,
    pub sk_rate_limit_per_minute: Option<i32>,

    pub platform_fingerprint: PlatformConfigVersion,
}

impl From<ApplicationKeyView> for CachedApplicationKeyView {
    fn from(a: ApplicationKeyView) -> Self {
        let platform_fingerprint = IntegrityConfigHandler::new_from_jsonb(&a.app_app_meta)
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
            sk_tier: a.sk_tier,
            sk_monthly_message_quota: a.sk_monthly_message_quota,
            sk_message_retention_seconds: a.sk_message_retention_seconds,
            sk_rate_limit_per_minute: a.sk_rate_limit_per_minute,

            platform_fingerprint,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
pub struct QuotaWarning {
    pub application_id: Option<Uuid>,
    pub application_name: Option<String>,
    #[schema(value_type = f64)]
    pub quota_usage_percentage: Option<Decimal>,
    pub current_month_messages_sent: Option<i64>,
    pub monthly_message_quota: Option<i64>,
    pub remaining_quota: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone, ToSchema)]
pub struct UserUsageSummary {
    // i32 fields corrected earlier
    pub total_applications: Option<i32>,
    pub active_applications: Option<i32>,

    // i64 fields need correction now
    pub total_messages_sent_current_month: Option<i64>,
    pub total_messages_received_current_month: Option<i64>,
    pub total_cost_cents_current_month: Option<i64>,
    pub total_lifetime_messages: Option<i64>,
    pub total_lifetime_cost_cents: Option<i64>,

    // Remaining i32 fields corrected earlier
    pub apps_over_80_percent_quota: Option<i32>,
    pub apps_over_quota: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow, ToSchema)]
pub struct ApplicationWithKeys {
    pub application_id: Uuid,
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

    #[schema(value_type = AppMetaData)]
    pub app_meta: Json<AppMetaData>,

    #[serde(skip_serializing)]
    pub secret_key_id: Option<Uuid>,

    // Publishable keys
    pub publishable_key_count: i64,

    #[schema(value_type = Vec<PublishableKey>)]
    pub publishable_keys: Json<Vec<PublishableKey>>,

    // Webhooks
    pub webhook_count: i64,

    #[schema(value_type = Vec<Webhook>)]
    pub webhooks: Json<Vec<Webhook>>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct PaginatedApplicationsSummary {
    pub data: Vec<ApplicationSummary>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, FromRow, ToSchema)]
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
    pub quota_usage_percentage: f64,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct PaginatedQuotaWarnings {
    pub data: Vec<QuotaWarning>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

pub fn secret_key_resolution_cache_key(key_hash: &str) -> String {
    cache_key!("res", "sk", key_hash)
}

pub fn publishable_key_resolution_cache_key(pk_plaintext: &str) -> String {
    cache_key!("res", "pk", pk_plaintext)
}
