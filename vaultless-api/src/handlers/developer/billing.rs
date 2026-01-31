use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use vaultless_core::{
    models::{
        billing::{
            CreditTransaction, DeveloperRevenueShare,
            ClientSubscription, ClientBillingUsage,
            ClientInvoice, BillingPeriod
        },
    },
};

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct GetBillingHistoryQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct GetUsageQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct GetRevenueQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BillingOverviewResponse {
    pub total_credits: i64,
    pub credit_balance: i64,
    pub credit_consumed: i64,
    pub credit_provided: i64,
    pub recent_transactions: Vec<CreditTransaction>,
    pub active_subscriptions: Vec<ClientSubscription>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UsageReportResponse {
    pub period_start: chrono::DateTime<chrono::Utc>,
    pub period_end: chrono::DateTime<chrono::Utc>,
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_proofs_verified: i64,
    pub usage_records: Vec<ClientBillingUsage>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RevenueReportResponse {
    pub period_start: chrono::DateTime<chrono::Utc>,
    pub period_end: chrono::DateTime<chrono::Utc>,
    pub gross_revenue_cents: i64,
    pub platform_fees_cents: i64,
    pub net_revenue_cents: i64,
    pub revenue_shares: Vec<DeveloperRevenueShare>,
}

/// Get billing overview for the authenticated developer
#[utoipa::path(
    get,
    path = "/billing/overview",
    responses(
        (status = 200, description = "Billing overview retrieved successfully", body = BillingOverviewResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_billing_overview(
    State(state): State<AppState>,
    user_session: SessionDataUserExt,
) -> Result<Json<BillingOverviewResponse>, ApiError> {
    let user_id = user_session.0.user_id;
    
    // Get an application ID for the user (we need to find an application owned by this user)
    let app_result = sqlx::query!("SELECT id FROM applications WHERE developer_id = $1 LIMIT 1", user_id)
        .fetch_optional(&*state.db)
        .await
        .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    let client_id = if let Some(app_row) = app_result {
        // If we found an application, we can get the associated client ID
        // For this simplified version, we'll use the user ID as the client ID
        user_id
    } else {
        // If no application found, return an empty response
        let response = BillingOverviewResponse {
            total_credits: 0,
            credit_balance: 0,
            credit_consumed: 0,
            credit_provided: 0,
            recent_transactions: vec![],
            active_subscriptions: vec![],
        };
        return Ok(Json(response));
    };
    
    // Use the correct field name for the database connection
    let pool = &*state.db;
    
    // Get client's credit balance
    let client_credit = sqlx::query_as::<_, vaultless_core::models::billing::ClientUsageCredit>(
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
    
    // Get active subscriptions for the user
    let active_subscriptions = sqlx::query_as::<_, ClientSubscription>(
        "SELECT * FROM client_subscriptions WHERE client_id = $1 AND status = 'active'"
    )
    .bind(client_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    let response = BillingOverviewResponse {
        total_credits: client_credit.as_ref().map(|c| c.credit_provided).unwrap_or(0),
        credit_balance: client_credit.as_ref().map(|c| c.credit_balance).unwrap_or(0),
        credit_consumed: client_credit.as_ref().map(|c| c.credit_consumed).unwrap_or(0),
        credit_provided: client_credit.as_ref().map(|c| c.credit_provided).unwrap_or(0),
        recent_transactions,
        active_subscriptions,
    };
    
    Ok(Json(response))
}

/// Get billing history for the authenticated developer
#[utoipa::path(
    get,
    path = "/billing/history",
    params(GetBillingHistoryQuery),
    responses(
        (status = 200, description = "Billing history retrieved successfully", body = [CreditTransaction]),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_billing_history(
    State(state): State<AppState>,
    user_session: SessionDataUserExt,
    Query(query): Query<GetBillingHistoryQuery>,
) -> Result<Json<Vec<CreditTransaction>>, ApiError> {
    let user_id = user_session.0.user_id;
    
    // Get an application ID for the user (we need to find an application owned by this user)
    let app_result = sqlx::query!("SELECT id FROM applications WHERE developer_id = $1 LIMIT 1", user_id)
        .fetch_optional(&*state.db)
        .await
        .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    let client_id = if let Some(_app_row) = app_result {
        // For this simplified version, we'll use the user ID as the client ID
        user_id
    } else {
        // If no application found, return an empty list
        return Ok(Json(vec![]));
    };
    
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

/// Get usage report for the authenticated developer
#[utoipa::path(
    get,
    path = "/billing/usage",
    params(GetUsageQuery),
    responses(
        (status = 200, description = "Usage report retrieved successfully", body = UsageReportResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_usage_report(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Query(query): Query<GetUsageQuery>,
) -> Result<Json<UsageReportResponse>, ApiError> {
    let user_id = session.user_id;
    
    // Get an application ID for the user (we need to find an application owned by this user)
    let app_result = sqlx::query!("SELECT id FROM applications WHERE developer_id = $1 LIMIT 1", user_id)
        .fetch_optional(&*state.db)
        .await
        .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    let client_id = if let Some(_app_row) = app_result {
        // For this simplified version, we'll use the user ID as the client ID
        user_id
    } else {
        // If no application found, return an empty response
        let response = UsageReportResponse {
            period_start: chrono::Utc::now() - chrono::Duration::days(30),
            period_end: chrono::Utc::now(),
            total_messages_sent: 0,
            total_messages_received: 0,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            total_proofs_verified: 0,
            usage_records: vec![],
        };
        return Ok(Json(response));
    };
    
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
    
    let response = UsageReportResponse {
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

/// Get revenue report for the authenticated developer
#[utoipa::path(
    get,
    path = "/billing/revenue",
    params(GetRevenueQuery),
    responses(
        (status = 200, description = "Revenue report retrieved successfully", body = RevenueReportResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_revenue_report(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Query(query): Query<GetRevenueQuery>,
) -> Result<Json<RevenueReportResponse>, ApiError> {
    let user_id = session.user_id;
    
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
    
    // Get revenue shares for the specified period
    let revenue_shares = sqlx::query_as::<_, DeveloperRevenueShare>(
        "SELECT * FROM developer_revenue_shares WHERE developer_id = $1 AND calculated_at BETWEEN $2 AND $3 ORDER BY calculated_at DESC"
    )
    .bind(user_id)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    // Aggregate revenue data
    let mut gross_revenue_cents = 0i64;
    let mut platform_fees_cents = 0i64;
    let mut net_revenue_cents = 0i64;
    
    for share in &revenue_shares {
        gross_revenue_cents += share.gross_revenue_cents;
        platform_fees_cents += share.platform_fee_cents;
        net_revenue_cents += share.net_revenue_cents;
    }
    
    let response = RevenueReportResponse {
        period_start: start_date.with_timezone(&chrono::Utc),
        period_end: end_date.with_timezone(&chrono::Utc),
        gross_revenue_cents,
        platform_fees_cents,
        net_revenue_cents,
        revenue_shares,
    };
    
    Ok(Json(response))
}

/// Get current billing period for the authenticated developer
#[utoipa::path(
    get,
    path = "/billing/current-period",
    responses(
        (status = 200, description = "Current billing period retrieved successfully", body = BillingPeriod),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_current_billing_period(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
) -> Result<Json<Option<BillingPeriod>>, ApiError> {
    let user_id = session.user_id;
    
    // First, get an application ID for the user (we need to find an application owned by this user)
    let app_result = sqlx::query!("SELECT id FROM applications WHERE developer_id = $1 LIMIT 1", user_id)
        .fetch_optional(&*state.db)
        .await
        .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    if let Some(app_row) = app_result {
        let billing_period = sqlx::query_as::<_, BillingPeriod>(
            "SELECT * FROM billing_periods WHERE application_id = $1 AND $2 BETWEEN period_start AND period_end LIMIT 1"
        )
        .bind(app_row.id)
        .bind(chrono::Utc::now())
        .fetch_optional(&*state.db)
        .await
        .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
        
        Ok(Json(billing_period))
    } else {
        // If no application found, return None
        Ok(Json(None))
    }
}

/// Get invoices for the authenticated developer
#[utoipa::path(
    get,
    path = "/billing/invoices",
    responses(
        (status = 200, description = "Invoices retrieved successfully", body = [ClientInvoice]),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_invoices(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
) -> Result<Json<Vec<ClientInvoice>>, ApiError> {
    let user_id = session.user_id;
    
    // Get an application ID for the user (we need to find an application owned by this user)
    let app_result = sqlx::query!("SELECT id FROM applications WHERE developer_id = $1 LIMIT 1", user_id)
        .fetch_optional(&*state.db)
        .await
        .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    let client_id = if let Some(_app_row) = app_result {
        // For this simplified version, we'll use the user ID as the client ID
        user_id
    } else {
        // If no application found, return an empty list
        return Ok(Json(vec![]));
    };
    
    let invoices = sqlx::query_as::<_, ClientInvoice>(
        "SELECT * FROM client_invoices WHERE client_id = $1 ORDER BY created_at DESC"
    )
    .bind(client_id)
    .fetch_all(&*state.db)
    .await
    .map_err(|e| ApiError::internal_server_error(&format!("Database error: {}", e)))?;
    
    Ok(Json(invoices))
}