use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use vaultless_core::{
    models::{
        billing::{
            ClientUsageCredit, CreditTransaction, ClientBillingUsage,
            ClientInvoice
        },
    },
};

use crate::{
    middleware::{error::ApiError, client::SessionDataClientExt},
    state::AppState,
};

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct GetClientBillingHistoryQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct GetClientUsageQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ClientBillingOverviewResponse {
    pub total_credits: i64,
    pub credit_balance: i64,
    pub credit_consumed: i64,
    pub credit_provided: i64,
    pub recent_transactions: Vec<CreditTransaction>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ClientUsageReportResponse {
    pub period_start: chrono::DateTime<chrono::Utc>,
    pub period_end: chrono::DateTime<chrono::Utc>,
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_proofs_verified: i64,
    pub usage_records: Vec<ClientBillingUsage>,
}

/// Get billing overview for the authenticated client
#[utoipa::path(
    get,
    path = "/api/clients/billing/overview",
    responses(
        (status = 200, description = "Client billing overview retrieved successfully", body = ClientBillingOverviewResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_billing_overview(
    State(state): State<AppState>,
    client_session: SessionDataClientExt,
) -> Result<Json<ClientBillingOverviewResponse>, ApiError> {
    let client_id = client_session.0.client_id;
    
    let pool = &*state.db;
    
    // Get client's credit balance
    let client_credit = sqlx::query_as::<_, ClientUsageCredit>(
        "SELECT * FROM client_usage_credits WHERE client_id = $1"
    )
    .bind(client_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    // Get recent credit transactions
    let recent_transactions = sqlx::query_as::<_, CreditTransaction>(
        "SELECT * FROM credit_transactions WHERE client_id = $1 ORDER BY created_at DESC LIMIT 10"
    )
    .bind(client_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    let response = ClientBillingOverviewResponse {
        total_credits: client_credit.as_ref().map(|c| c.credit_provided).unwrap_or(0),
        credit_balance: client_credit.as_ref().map(|c| c.credit_balance).unwrap_or(0),
        credit_consumed: client_credit.as_ref().map(|c| c.credit_consumed).unwrap_or(0),
        credit_provided: client_credit.as_ref().map(|c| c.credit_provided).unwrap_or(0),
        recent_transactions,
    };
    
    Ok(Json(response))
}

/// Get billing history for the authenticated client
#[utoipa::path(
    get,
    path = "/api/clients/billing/history",
    params(GetClientBillingHistoryQuery),
    responses(
        (status = 200, description = "Client billing history retrieved successfully", body = [CreditTransaction]),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_billing_history(
    State(state): State<AppState>,
    SessionDataClientExt(session): SessionDataClientExt,
    Query(query): Query<GetClientBillingHistoryQuery>,
) -> Result<Json<Vec<CreditTransaction>>, ApiError> {
    let client_id = session.client_id;
    
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(20).min(100); // Max 100 per page
    
    let offset = (page - 1) * page_size;
    
    let transactions = sqlx::query_as::<_, CreditTransaction>(
        "SELECT * FROM credit_transactions WHERE client_id = $1 ORDER BY created_at DESC OFFSET $2 LIMIT $3"
    )
    .bind(client_id)
    .bind(offset)
    .bind(page_size)
    .fetch_all(&*state.db)
    .await
    .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    Ok(Json(transactions))
}

/// Get usage report for the authenticated client
#[utoipa::path(
    get,
    path = "/api/clients/billing/usage",
    params(GetClientUsageQuery),
    responses(
        (status = 200, description = "Client usage report retrieved successfully", body = ClientUsageReportResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_usage_report(
    State(state): State<AppState>,
    SessionDataClientExt(session): SessionDataClientExt,
    Query(query): Query<GetClientUsageQuery>,
) -> Result<Json<ClientUsageReportResponse>, ApiError> {
    let client_id = session.client_id;

    let pool = &*state.db;

    // Determine the date range for the query
    let start_date = query.start_date.and_then(|date_str| {
        chrono::DateTime::parse_from_rfc3339(&date_str).ok()
    }).unwrap_or_else(|| {
        (chrono::Utc::now() - chrono::Duration::days(30)).into() // Default to last 30 days
    });

    let end_date = query.end_date.and_then(|date_str| {
        chrono::DateTime::parse_from_rfc3339(&date_str).ok()
    }).unwrap_or_else(|| chrono::Utc::now().into());

    // Get usage records for the specified period
    let usage_records = sqlx::query_as::<_, ClientBillingUsage>(
        "SELECT * FROM client_billing_usage WHERE client_id = $1 AND created_at BETWEEN $2 AND $3 ORDER BY created_at DESC"
    )
    .bind(client_id)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;

    // Aggregate usage data
    let mut total_messages_sent = 0i64;
    let mut total_messages_received = 0i64;
    let mut total_bytes_sent = 0i64;
    let mut total_bytes_received = 0i64;
    let mut total_proofs_verified = 0i64;

    for record in &usage_records {
        total_messages_sent += record.messages_sent;
        total_messages_received += record.messages_received;
        total_bytes_sent += record.total_bytes_sent;
        total_bytes_received += record.total_bytes_received;
        total_proofs_verified += record.proofs_verified;
    }

    let response = ClientUsageReportResponse {
        period_start: start_date.with_timezone(&chrono::Utc),
        period_end: end_date.with_timezone(&chrono::Utc),
        total_messages_sent,
        total_messages_received,
        total_bytes_sent,
        total_bytes_received,
        total_proofs_verified,
        usage_records,
    };

    Ok(Json(response))
}