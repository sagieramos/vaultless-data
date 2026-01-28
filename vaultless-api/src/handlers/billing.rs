use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;
use vaultless_core::{
    error::Result,
    models::{ClientUsageCredit, CreditTransaction},
    services::billing::BillingService,
};

#[derive(Debug, Deserialize)]
pub struct ProcessUsageRequest {
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub messages_sent: i64,
    pub messages_received: i64,
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub proofs_verified: i64,
}

#[derive(Debug, Serialize)]
pub struct ProcessUsageResponse {
    pub success: bool,
    pub remaining_credits: i64,
}

/// Handler for processing usage events
/// This implements the mental model: Credits unlock usage -> Usage creates entitlement -> PSP moves money
pub async fn process_usage_event(
    State(state): State<AppState>,
    Json(payload): Json<ProcessUsageRequest>,
) -> Result<(StatusCode, Json<ProcessUsageResponse>), StatusCode> {
    let mut tx = state.pool.begin().await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get the developer ID associated with this application
    let app_result = sqlx::query!(
        r#"
        SELECT developer_id FROM applications WHERE id = $1
        "#,
        payload.application_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let developer_id = match app_result {
        Some(app) => app.developer_id,
        None => return Err(StatusCode::BAD_REQUEST),
    };

    // Get the current billing period for this application
    let billing_period_result = sqlx::query!(
        r#"
        SELECT id FROM billing_periods 
        WHERE application_id = $1 AND status = 'open'
        "#,
        payload.application_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let billing_period_id = match billing_period_result {
        Some(bp) => bp.id,
        None => return Err(StatusCode::BAD_REQUEST),
    };

    // Get the pricing plan for this application
    // In a real implementation, this would come from the application's pricing plan
    use vaultless_core::models::pricing::snapshot::{PricingSnapshot, PricingMode};
    let pricing_snapshot = PricingSnapshot {
        plan_id: Uuid::nil(), // Placeholder
        plan_name: "Standard".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(1), // $0.01 per message
        price_per_gb_cents: Some(1000),   // $10.00 per GB
        price_per_proof_cents: Some(5),   // $0.05 per proof
        prepaid_amount_cents: None,
    };

    // Process the usage event (this handles credits and revenue attribution)
    BillingService::process_usage_event(
        &mut tx,
        payload.client_id,
        payload.application_id,
        developer_id,
        &pricing_snapshot,
        payload.messages_sent,
        payload.messages_received,
        payload.bytes_sent,
        payload.bytes_received,
        payload.proofs_verified,
        billing_period_id,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get the remaining credits for the response
    let client_credit = ClientUsageCredit::find_by_client(
        &mut *tx,
        payload.client_id,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let remaining_credits = client_credit.map(|cc| cc.credit_balance).unwrap_or(0);

    tx.commit().await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        StatusCode::OK,
        Json(ProcessUsageResponse {
            success: true,
            remaining_credits,
        }),
    ))
}

#[derive(Debug, Serialize)]
pub struct GetClientCreditsResponse {
    pub credit_balance: i64,
    pub credit_consumed: i64,
    pub credit_provided: i64,
    pub estimated_cash_value_cents: i64,
    pub transactions: Vec<CreditTransaction>,
}

/// Handler to get client credit information
pub async fn get_client_credits(
    State(state): State<AppState>,
    Path(client_id): Path<Uuid>,
) -> Result<(StatusCode, Json<GetClientCreditsResponse>), StatusCode> {
    // Get the client's credit information
    let credit_info = sqlx::query_as!(
        ClientUsageCredit,
        r#"
        SELECT
            id,
            client_id,
            credit_balance,
            credit_consumed,
            credit_provided,
            estimated_cash_value_cents,
            expires_at,
            created_at,
            updated_at
        FROM client_usage_credits
        WHERE client_id = $1
        "#,
        client_id
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let credit_info = match credit_info {
        Some(credit) => credit,
        None => return Err(StatusCode::NOT_FOUND),
    };

    // Get recent transactions for this client
    let transactions = CreditTransaction::find_by_client(&state.pool, client_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        StatusCode::OK,
        Json(GetClientCreditsResponse {
            credit_balance: credit_info.credit_balance,
            credit_consumed: credit_info.credit_consumed,
            credit_provided: credit_info.credit_provided,
            estimated_cash_value_cents: credit_info.estimated_cash_value_cents,
            transactions,
        }),
    ))
}