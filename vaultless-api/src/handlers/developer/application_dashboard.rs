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
use serde_json::Value;
use vaultless_core::models::app_model::dto::ApplicationWithUsageResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    pub active: bool,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub max_ttl: i32,
    pub rotation_forced: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub meta: AppMetaData,

    // Secret key tier info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_quota: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_seconds: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhooks: Option<Value>,

    pub quota_usage_pct: f64,

    pub current_month: Usage,
    pub lifetime: Usage,
    pub last_7d: Trend,
    pub last_30d: Trend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub msg_sent: i64,
    pub msg_received: i64,
    pub msg_proof: i64,
    pub msg_stored: i64,
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub rate_hits: i64,
    pub cost: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trend {
    pub msg_sent: i64,
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub cost: i64,
}

impl From<ApplicationWithUsageResponse> for ApplicationResponse {
    fn from(app: ApplicationWithUsageResponse) -> Self {
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
            keys: Some(app.publishable_keys),
            webhooks: Some(app.webhooks),
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
