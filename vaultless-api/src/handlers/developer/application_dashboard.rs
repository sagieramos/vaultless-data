use crate::{
    AppState,
    middleware::{error::ApiError, user::SessionDataUserExt},
};
use axum::{
    extract::{Path, State},
    response::Json,
};
use uuid::Uuid;
use vaultless_core::{Application, models::app_model::integrity::dto::AppMetaData};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use vaultless_core::models::app_model::dto::ApplicationWithUsage;
use vaultless_core::models::app_model::dto::{PublishableKey, Webhook};

// FIX: Added `ToSchema` here
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApplicationResponse {
    /// Application unique identifier
    #[schema(value_type = String)]
    pub id: Uuid,
    /// Application name
    pub name: String,
    /// Application description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    /// Whether the application is active
    pub active: bool,
    /// Creation timestamp
    #[schema(value_type = String)]
    pub created: DateTime<Utc>,
    /// Last update timestamp
    #[schema(value_type = String)]
    pub updated: DateTime<Utc>,
    /// Maximum time-to-live in seconds
    pub max_ttl: i32,
    /// Whether key rotation is forced
    pub rotation_forced: bool,
    /// Deletion requested timestamp (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub deleted_at: Option<DateTime<Utc>>,
    /// Application metadata
    pub meta: AppMetaData,

    // Secret key tier info
    /// Subscription tier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Monthly message quota
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_quota: Option<i64>,
    /// Rate limit per minute
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<i32>,
    /// Message retention in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_seconds: Option<i32>,

    /// List of publishable keys
    #[schema(value_type = Vec<Object>)]
    pub keys: Vec<PublishableKey>,
    /// List of webhooks
    #[schema(value_type = Vec<Object>)]
    pub webhooks: Vec<Webhook>,

    /// Quota usage percentage
    pub quota_usage_pct: f64,

    /// Current month usage statistics
    pub current_month: Usage,
    /// Lifetime usage statistics
    pub lifetime: Usage,
    /// Last 7 days trend
    pub last_7d: Trend,
    /// Last 30 days trend
    pub last_30d: Trend,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Usage {
    /// Messages sent
    pub msg_sent: i64,
    /// Messages received
    pub msg_received: i64,
    /// Messages with proof verification
    pub msg_proof: i64,
    /// Bytes stored
    pub msg_stored: i64,
    /// Bytes sent
    pub bytes_sent: i64,
    /// Bytes received
    pub bytes_received: i64,
    /// Rate limit hits
    pub rate_hits: i64,
    /// Cost in cents
    pub cost: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Trend {
    /// Messages sent
    pub msg_sent: i64,
    /// Bytes sent
    pub bytes_sent: i64,
    /// Bytes received
    pub bytes_received: i64,
    /// Cost in cents
    pub cost: i64,
}

impl From<ApplicationWithUsage> for ApplicationResponse {
    fn from(app: ApplicationWithUsage) -> Self {
        Self {
            id: app.application_id,
            name: app.name,
            desc: app.description,
            active: app.is_active,
            created: app.created_at,
            updated: app.updated_at,
            max_ttl: app.max_ttl_seconds,
            rotation_forced: app.is_key_rotation_forced,
            deleted_at: app.deletion_requested_at,
            meta: app.app_meta.0,
            tier: app.tier,
            monthly_quota: app.monthly_message_quota,
            rate_limit: app.rate_limit_per_minute,
            retention_seconds: app.message_retention_seconds,
            keys: app.publishable_keys.0,
            webhooks: app.webhooks.0,
            quota_usage_pct: app.quota_usage_percentage,
            current_month: Usage {
                msg_sent: app.current_month_messages_sent,
                msg_received: app.current_month_messages_received,
                msg_proof: app.current_month_proofs_verified,
                msg_stored: app.current_month_bytes_stored,
                bytes_sent: app.current_month_bytes_sent,
                bytes_received: app.current_month_bytes_received,
                rate_hits: app.current_month_rate_limit_hits,
                cost: app.current_month_cost_cents,
            },
            lifetime: Usage {
                msg_sent: app.lifetime_messages_sent,
                msg_received: app.lifetime_messages_received,
                msg_proof: app.lifetime_proofs_verified,
                msg_stored: app.lifetime_bytes_stored,
                bytes_sent: app.lifetime_bytes_sent,
                bytes_received: app.lifetime_bytes_received,
                rate_hits: app.lifetime_rate_limit_hits,
                cost: app.lifetime_cost_cents,
            },
            last_7d: Trend {
                msg_sent: app.last_7d_messages_sent,
                bytes_sent: app.last_7d_bytes_sent,
                bytes_received: app.last_7d_bytes_received,
                cost: app.last_7d_cost_cents,
            },
            last_30d: Trend {
                msg_sent: app.last_30d_messages_sent,
                bytes_sent: app.last_30d_bytes_sent,
                bytes_received: app.last_30d_bytes_received,
                cost: app.last_30d_cost_cents,
            },
        }
    }
}

/// GET /api/v1/applications/:application_id/analytics
/// Full analytics endpoint for dashboards or heavy reporting
#[utoipa::path(
    get,
    path = "/api/v1/applications/{application_id}/analytics",
    params(
        ("application_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Application details with usage data", body = ApplicationResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "developer"
)]
pub async fn get_application_with_keys_handler(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(application_id): Path<Uuid>,
) -> Result<Json<ApplicationResponse>, ApiError> {
    // Use existing method to fetch application + usage
    let app_row =
        Application::find_owned_by_user(state.db.as_ref(), application_id, session.user_id)
            .await
            .map_err(ApiError::from)?;

    // Convert to full analytics response
    let response: ApplicationResponse = app_row.into();

    Ok(Json(response))
}
