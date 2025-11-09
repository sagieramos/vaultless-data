use crate::cache_key;

use crate::types::SubscriptionTier;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationValidation {
    pub is_valid: bool,
    pub is_active: bool,
    pub api_key_active: bool,
    pub tier: Option<SubscriptionTier>,
    pub quota_status: Option<QuotaStatus>,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    pub limit: i64,
    pub used: i64,
    pub remaining: i64,
    pub percentage_used: f64,
    pub is_exceeded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
    pub severity: ErrorSeverity,
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
    pub publishable_key: String,
    pub publishable_key_prefix: String,
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

    #[validate(length(max = 255))]
    pub bundle_id: Option<String>,

    #[validate(length(max = 50))]
    pub platform: Option<String>,

    #[validate(url)]
    pub webhook_url: Option<String>,
}

/// Application with denormalized tier information from api_keys
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApplicationWithTier {
    // All Application fields
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub secret_key_id: Uuid,
    pub publishable_key: String,
    pub publishable_key_prefix: String,
    pub bundle_id: Option<String>,
    pub platform: Option<String>,
    pub webhook_url: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Tier information from api_keys JOIN
    pub tier: SubscriptionTier,
    pub monthly_message_quota: i32,
    pub rate_limit_per_minute: i32,
    pub message_retention_seconds: i32,
    pub api_key_active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateApplicationResponse {
    pub application: Application,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>, // Only returned once at creation
    pub publishable_key: String,
}

pub fn cache_key_by_id(id: Uuid) -> String {
    cache_key!("app", "id", id)
}

pub fn cache_key_by_publishable_key(pk: &str) -> String {
    cache_key!("app", "pk", pk)
}
