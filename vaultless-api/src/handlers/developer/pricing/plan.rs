//! Pricing plan handlers for developers.
//!
//! Provides endpoints for:
//! - Creating pricing plans
//! - Retrieving pricing plans
//! - Updating pricing plans
//! - Deleting pricing plans

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    response::Json,
    Json as AxumJson,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use vaultless_core::{
    models::pricing::{
        dto::CreatePricingPlan,
        enums::PricingMode,
        pricing_plan::{Paginated, PricingPlan},
    },
};

// ============================================================================
// REQUEST/RESPONSE DTOs
// ============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePricingPlanRequest {
    pub name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PricingPlanResponse {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub attached_app_count: i64,
}

impl From<vaultless_core::models::pricing::pricing_plan::PricingPlanWithAttachmentCount> for PricingPlanResponse {
    fn from(plan_with_count: vaultless_core::models::pricing::pricing_plan::PricingPlanWithAttachmentCount) -> Self {
        Self {
            id: plan_with_count.id,
            developer_id: plan_with_count.developer_id,
            name: plan_with_count.name,
            pricing_mode: plan_with_count.pricing_mode,
            price_per_message_cents: plan_with_count.price_per_message_cents,
            price_per_gb_cents: plan_with_count.price_per_gb_cents,
            price_per_proof_cents: plan_with_count.price_per_proof_cents,
            prepaid_amount_cents: plan_with_count.prepaid_amount_cents,
            created_at: plan_with_count.created_at,
            attached_app_count: plan_with_count.attached_app_count,
        }
    }
}

impl From<vaultless_core::models::pricing::pricing_plan::PricingPlan> for PricingPlanResponse {
    fn from(plan: vaultless_core::models::pricing::pricing_plan::PricingPlan) -> Self {
        Self {
            id: plan.id,
            developer_id: plan.developer_id,
            name: plan.name,
            pricing_mode: plan.pricing_mode,
            price_per_message_cents: plan.price_per_message_cents,
            price_per_gb_cents: plan.price_per_gb_cents,
            price_per_proof_cents: plan.price_per_proof_cents,
            prepaid_amount_cents: plan.prepaid_amount_cents,
            created_at: plan.created_at,
            attached_app_count: 0, // New plans have no attachments
        }
    }
}

impl From<vaultless_core::models::pricing::pricing_plan::PricingPlanWithAttachedApps> for PricingPlanResponse {
    fn from(plan: vaultless_core::models::pricing::pricing_plan::PricingPlanWithAttachedApps) -> Self {
        Self {
            id: plan.id,
            developer_id: plan.developer_id,
            name: plan.name,
            pricing_mode: plan.pricing_mode,
            price_per_message_cents: plan.price_per_message_cents,
            price_per_gb_cents: plan.price_per_gb_cents,
            price_per_proof_cents: plan.price_per_proof_cents,
            prepaid_amount_cents: plan.prepaid_amount_cents,
            created_at: plan.created_at,
            attached_app_count: plan.attached_app_count,
        }
    }
}

impl CreatePricingPlanRequest {
    fn into_create_pricing_plan(self, developer_id: Uuid) -> CreatePricingPlan {
        CreatePricingPlan {
            developer_id,
            name: self.name,
            pricing_mode: self.pricing_mode,
            price_per_message_cents: self.price_per_message_cents,
            price_per_gb_cents: self.price_per_gb_cents,
            price_per_proof_cents: self.price_per_proof_cents,
            prepaid_amount_cents: self.prepaid_amount_cents,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct PricingPlansQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 {
    1
}

fn default_page_size() -> i64 {
    20
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletePricingPlanResponse {
    pub success: bool,
    pub deleted_count: u64,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// Create a new pricing plan for the developer
#[utoipa::path(
    post,
    path = "/dev/pricing/plans",
    request_body = CreatePricingPlanRequest,
    responses(
        (status = 201, description = "Pricing plan created successfully", body = PricingPlanResponse),
        (status = 400, description = "Invalid request data"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "pricing"
)]
pub async fn create_pricing_plan(
    SessionDataUserExt(session): SessionDataUserExt,
    State(state): State<AppState>,
    AxumJson(request): AxumJson<CreatePricingPlanRequest>,
) -> Result<Json<PricingPlanResponse>, ApiError> {
    let input = request.into_create_pricing_plan(session.user_id);
    let plan = PricingPlan::create(state.db.as_ref(), input).await?;
    Ok(Json(PricingPlanResponse::from(plan)))
}

/// Get a specific pricing plan by ID
#[utoipa::path(
    get,
    path = "/dev/pricing/plans/{plan_id}",
    params(
        ("plan_id" = Uuid, Path, description = "Pricing Plan ID")
    ),
    responses(
        (status = 200, description = "Pricing plan retrieved successfully", body = PricingPlanResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Pricing plan not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "pricing"
)]
pub async fn get_pricing_plan(
    Path(plan_id): Path<Uuid>,
    SessionDataUserExt(session): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<PricingPlanResponse>, ApiError> {
    let plan = PricingPlan::find_by_id_with_attached_apps(state.db.as_ref(), session.user_id, plan_id, None).await?;
    Ok(Json(PricingPlanResponse::from(plan)))
}

/// Get all pricing plans for the developer
#[utoipa::path(
    get,
    path = "/dev/pricing/plans",
    params(
        PricingPlansQuery
    ),
    responses(
        (status = 200, description = "Pricing plans retrieved successfully", body = Paginated<PricingPlanResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "pricing"
)]
pub async fn get_pricing_plans(
    SessionDataUserExt(session): SessionDataUserExt,
    Query(query): Query<PricingPlansQuery>,
    State(state): State<AppState>,
) -> Result<Json<Paginated<PricingPlanResponse>>, ApiError> {
    let paginated_plans = PricingPlan::find_with_attachment_count(
        state.db.as_ref(),
        session.user_id,
        None,
        Some(query.page),
        Some(query.page_size)
    ).await?;

    let plan_responses: Vec<PricingPlanResponse> = paginated_plans.items.into_iter().map(PricingPlanResponse::from).collect();

    Ok(Json(Paginated {
        items: plan_responses,
        total_count: paginated_plans.total_count,
        total_pages: paginated_plans.total_pages,
        page: paginated_plans.page,
        page_size: paginated_plans.page_size,
    }))
}

/// Delete a pricing plan by ID
#[utoipa::path(
    delete,
    path = "/dev/pricing/plans/{plan_id}",
    params(
        ("plan_id" = Uuid, Path, description = "Pricing Plan ID")
    ),
    responses(
        (status = 200, description = "Pricing plan deleted successfully", body = DeletePricingPlanResponse),
        (status = 400, description = "Cannot delete pricing plan attached to applications"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Pricing plan not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "pricing"
)]
pub async fn delete_pricing_plan(
    Path(plan_id): Path<Uuid>,
    SessionDataUserExt(session): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<DeletePricingPlanResponse>, ApiError> {
    let deleted = PricingPlan::delete(state.db.as_ref(), plan_id, session.user_id).await?;

    Ok(Json(DeletePricingPlanResponse {
        success: deleted,
        deleted_count: if deleted { 1 } else { 0 },
    }))
}