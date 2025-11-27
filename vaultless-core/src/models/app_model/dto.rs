use crate::cache_key;
use crate::types::SubscriptionTier;
use bigdecimal::BigDecimal as Decimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::clone::Clone;
use std::fmt::Debug;
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
    pub integrity_config: serde_json::Value,
    pub publishable_keys: serde_json::Value,
    pub publishable_key_count: i64,
    pub webhooks: serde_json::Value,
    pub webhook_count: i64,
    pub total_count: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct ApplicationKeysInfo {
    pub secret_key_id: Uuid,
    pub publishable_keys: serde_json::Value,
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
    pub integrity_config: serde_json::Value,
    pub publishable_keys: serde_json::Value,
    pub webhooks: serde_json::Value,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApplicationWithUsageResponse {
    pub application_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub deletion_requested_at: Option<DateTime<Utc>>,
    pub integrity_config: serde_json::Value,

    // Secret key tier info
    pub tier: Option<String>,
    pub monthly_message_quota: Option<i32>,
    pub rate_limit_per_minute: Option<i32>,
    pub message_retention_seconds: Option<i32>,

    pub publishable_keys: serde_json::Value,
    pub webhooks: serde_json::Value,

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

    // Trend usage
    pub last_7d_messages_sent: i64,
    pub last_7d_bytes_sent: i64,
    pub last_7d_bytes_received: i64,
    pub last_7d_cost_cents: i64,

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
pub struct CachedAuthConfig {
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

            sk_id: a.sk_id,
            sk_tier: a.sk_tier,
            sk_monthly_message_quota: a.sk_monthly_message_quota,
            sk_message_retention_seconds: a.sk_message_retention_seconds,
            sk_rate_limit_per_minute: a.sk_rate_limit_per_minute,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct QuotaWarning {
    pub application_id: Option<Uuid>,
    pub application_name: Option<String>,
    pub quota_usage_percentage: Option<Decimal>,
    pub current_month_messages_sent: Option<i64>,
    pub monthly_message_quota: Option<i64>,
    pub remaining_quota: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
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
    pub integrity_config: Option<serde_json::Value>,
    pub secret_key_id: Option<Uuid>,

    pub publishable_keys: serde_json::Value,
    pub webhooks: serde_json::Value,
}

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
