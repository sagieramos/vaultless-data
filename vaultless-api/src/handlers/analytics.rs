use axum::{Json, extract::State};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vaultless_core::{ApiKey, DailyUsageSummary, MonthlyTotal, WeeklyUsageSummary};

use crate::{middleware::error::ApiError, services::token::SessionData, state::AppState};

// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================

#[derive(Debug, Serialize)]
pub struct AnalyticsDashboard {
    pub api_key_id: String,
    pub key_prefix: String,
    pub description: Option<String>,
    pub tier: String,
    pub current_month: MonthlyTotal,
    pub last_7_days: Vec<DailyUsageSummary>,
    pub last_4_weeks: Vec<WeeklyUsageSummary>,
    pub quota_usage: QuotaUsage,
    pub trends: vaultless_core::UsageTrends,
}

#[derive(Debug, Serialize)]
pub struct QuotaUsage {
    pub monthly_quota: i32,
    pub messages_used: i64,
    pub percentage_used: f64,
    pub remaining: i64,
    pub will_exceed: bool,
}

#[derive(Debug, Serialize)]
pub struct MessageStats {
    pub api_key_id: String,
    pub key_prefix: String,
    pub total_messages: i64,
    pub total_delivered: i64,
    pub total_expired: i64,
    pub average_access_count: f64,
    pub total_proofs_created: i64,
    pub total_proofs_verified: i64,
}

#[derive(Debug, Deserialize)]
pub struct RealtimeUsageQuery {
    pub since: DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct DateRangeQuery {
    pub start: String,
    pub end: String,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Serialize)]
pub struct PaginationInfo {
    pub page: i64,
    pub per_page: i64,
    pub total_items: i64,
    pub total_pages: i64,
    pub has_next: bool,
    pub has_prev: bool,
}

pub struct AnalyticsRequest {
    pub api_key_id: Option<Uuid>,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// Get analytics dashboard for user's API key
/// GET /api/v1/analytics/dashboard?api_key_id=<uuid>
pub async fn get_dashboard(
    State(state): State<AppState>,
    session: SessionData,
    Json(payload): Json<AnalyticsRequest>,
) -> Result<Json<Vec<AnalyticsDashboard>>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    tracing::info!(
        user_id = %user_id,
        "Fetching analytics dashboard"
    );

    // Get user's API keys
    let api_keys = if let Some(key_id) = payload.api_key_id {
        let key = ApiKey::find_by_id(&state.db, key_id)
            .await
            .map_err(ApiError::from)?;

        // Verify ownership
        if key.user_id != user_id {
            return Err(ApiError::forbidden("You don't own this API key"));
        }

        vec![key]
    } else {
        ApiKey::find_by_owner(&state.db, user_id)
            .await
            .map_err(ApiError::from)?
    };

    let mut dashboards = Vec::new();

    for api_key in api_keys {
        // Get current month total
        let current_month = DailyUsageSummary::get_current_month_total(&state.db, api_key.id)
            .await
            .map_err(ApiError::from)?;

        // Get last 7 days
        let last_7_days = DailyUsageSummary::get_last_n_days(&state.db, api_key.id, 7)
            .await
            .map_err(ApiError::from)?;

        // Get last 4 weeks
        let last_4_weeks = WeeklyUsageSummary::get_last_n_weeks(&state.db, api_key.id, 4)
            .await
            .map_err(ApiError::from)?;

        // Calculate quota usage
        let messages_used = current_month.total_messages_sent;
        let monthly_quota = api_key.monthly_message_quota as i64;
        let percentage_used = if monthly_quota > 0 {
            (messages_used as f64 / monthly_quota as f64) * 100.0
        } else {
            0.0
        };
        let remaining = monthly_quota - messages_used;
        let will_exceed = remaining < (monthly_quota / 10); // Less than 10% remaining

        let quota_usage = QuotaUsage {
            monthly_quota: api_key.monthly_message_quota,
            messages_used,
            percentage_used,
            remaining,
            will_exceed,
        };

        // Get usage trends
        let trends =
            vaultless_core::models::usage_timescale::get_usage_trends(&state.db, api_key.id)
                .await
                .map_err(ApiError::from)?;

        dashboards.push(AnalyticsDashboard {
            api_key_id: api_key.id.to_string(),
            key_prefix: api_key.key_prefix,
            current_month,
            last_7_days,
            last_4_weeks,
            quota_usage,
            trends,
        });
    }

    Ok(Json(dashboards))
}

/// Get real-time usage statistics
/// GET /api/v1/analytics/realtime?api_key_id=<uuid>&since=2025-01-01T00:00:00Z
pub async fn get_realtime_usage_stats(
    State(state): State<AppState>,
    session: SessionData,
    axum::extract::Query(params): axum::extract::Query<RealtimeUsageQuery>,
) -> Result<Json<Vec<MonthlyTotal>>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    let api_key_id = params
        .api_key_id
        .ok_or_else(|| ApiError::bad_request("api_key_id query parameter required"))?;

    // Verify ownership
    let api_key = ApiKey::find_by_id(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    if api_key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    let stats = vaultless_core::models::usage_timescale::get_realtime_usage(
        &state.db,
        api_key_id,
        params.since,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(vec![stats]))
}

/// Get daily usage for date range (paginated)
/// GET /api/v1/analytics/daily?api_key_id=<uuid>&start=2025-01-01&end=2025-01-31&page=1&per_page=10
pub async fn get_daily_usage(
    State(state): State<AppState>,
    session: SessionData,
    axum::extract::Query(params): axum::extract::Query<DateRangeQuery>,
) -> Result<Json<PaginatedResponse<DailyUsageSummary>>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    let api_key_id = params
        .api_key_id
        .ok_or_else(|| ApiError::bad_request("api_key_id query parameter required"))?;

    // Verify ownership
    let api_key = ApiKey::find_by_id(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    if api_key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    let start = params.start.parse().map_err(|_| {
        ApiError::bad_request("Invalid start date format. Use ISO 8601 (YYYY-MM-DD)")
    })?;

    let end = params
        .end
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid end date format. Use ISO 8601 (YYYY-MM-DD)"))?;

    // Pagination
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(30).min(100); // Max 100 per page
    let offset = (page - 1) * per_page;

    // Get total count
    let total_items: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT day)
        FROM usage_metrics_daily
        WHERE api_key_id = $1 AND day >= $2 AND day <= $3
        "#,
    )
    .bind(api_key_id)
    .bind(start)
    .bind(end)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // Get paginated data
    let summaries = sqlx::query_as::<_, DailyUsageSummary>(
        r#"
        SELECT * FROM usage_metrics_daily
        WHERE api_key_id = $1 AND day >= $2 AND day <= $3
        ORDER BY day DESC
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(api_key_id)
    .bind(start)
    .bind(end)
    .bind(per_page)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(ApiError::from)?;

    let total_pages = (total_items as f64 / per_page as f64).ceil() as i64;

    Ok(Json(PaginatedResponse {
        data: summaries,
        pagination: PaginationInfo {
            page,
            per_page,
            total_items,
            total_pages,
            has_next: page < total_pages,
            has_prev: page > 1,
        },
    }))
}

/// Get weekly usage for date range (paginated)
/// GET /api/v1/analytics/weekly?api_key_id=<uuid>&start=2025-01-01&end=2025-03-31&page=1&per_page=10
pub async fn get_weekly_usage(
    State(state): State<AppState>,
    session: SessionData,
    axum::extract::Query(params): axum::extract::Query<DateRangeQuery>,
) -> Result<Json<PaginatedResponse<WeeklyUsageSummary>>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    let api_key_id = params
        .api_key_id
        .ok_or_else(|| ApiError::bad_request("api_key_id query parameter required"))?;

    // Verify ownership
    let api_key = ApiKey::find_by_id(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    if api_key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    let start = params.start.parse().map_err(|_| {
        ApiError::bad_request("Invalid start date format. Use ISO 8601 (YYYY-MM-DD)")
    })?;

    let end = params
        .end
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid end date format. Use ISO 8601 (YYYY-MM-DD)"))?;

    // Pagination
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(12).min(52); // Max 52 weeks per page
    let offset = (page - 1) * per_page;

    // Get total count
    let total_items: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT week_start)
        FROM usage_metrics_weekly
        WHERE api_key_id = $1 AND week_start >= $2 AND week_start <= $3
        "#,
    )
    .bind(api_key_id)
    .bind(start)
    .bind(end)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // Get paginated data
    let summaries = sqlx::query_as::<_, WeeklyUsageSummary>(
        r#"
        SELECT * FROM usage_metrics_weekly
        WHERE api_key_id = $1 AND week_start >= $2 AND week_start <= $3
        ORDER BY week_start DESC
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(api_key_id)
    .bind(start)
    .bind(end)
    .bind(per_page)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(ApiError::from)?;

    let total_pages = (total_items as f64 / per_page as f64).ceil() as i64;

    Ok(Json(PaginatedResponse {
        data: summaries,
        pagination: PaginationInfo {
            page,
            per_page,
            total_items,
            total_pages,
            has_next: page < total_pages,
            has_prev: page > 1,
        },
    }))
}

/// Get message statistics
/// GET /api/v1/analytics/messages?api_key_id=<uuid>
pub async fn get_message_stats(
    State(state): State<AppState>,
    session: SessionData,
    axum::extract::Query(query): axum::extract::Query<AnalyticsQuery>,
) -> Result<Json<MessageStats>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    let api_key_id = query
        .api_key_id
        .ok_or_else(|| ApiError::bad_request("api_key_id query parameter required"))?;

    // Verify ownership
    let api_key = ApiKey::find_by_id(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    if api_key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    // Get message statistics
    let stats = sqlx::query_as::<_, MessageStatsRow>(
        r#"
        SELECT 
            COUNT(*) as total_messages,
            SUM(CASE WHEN is_delivered THEN 1 ELSE 0 END) as total_delivered,
            SUM(CASE WHEN expires_at < NOW() THEN 1 ELSE 0 END) as total_expired,
            AVG(access_count) as average_access_count
        FROM messages
        WHERE api_key_id = $1
        "#,
    )
    .bind(api_key_id)
    .fetch_one(&state.db)
    .await
    .map_err(ApiError::from)?;

    // Get proof statistics
    let proof_stats = sqlx::query_as::<_, ProofStatsRow>(
        r#"
        SELECT 
            COUNT(*) as total_proofs_created,
            SUM(CASE WHEN verified_at IS NOT NULL THEN 1 ELSE 0 END) as total_proofs_verified
        FROM message_proofs
        WHERE message_id IN (
            SELECT id FROM messages WHERE api_key_id = $1
        )
        "#,
    )
    .bind(api_key_id)
    .fetch_one(&state.db)
    .await
    .map_err(ApiError::from)?;

    Ok(Json(MessageStats {
        total_messages: stats.total_messages.unwrap_or(0),
        total_delivered: stats.total_delivered.unwrap_or(0),
        total_expired: stats.total_expired.unwrap_or(0),
        average_access_count: stats.average_access_count.unwrap_or(0.0),
        total_proofs_created: proof_stats.total_proofs_created.unwrap_or(0),
        total_proofs_verified: proof_stats.total_proofs_verified.unwrap_or(0),
    }))
}

/// Get usage statistics summary
/// GET /api/v1/analytics/usage
pub async fn get_usage_stats(
    State(state): State<AppState>,
    session: SessionData,
    axum::extract::Query(query): axum::extract::Query<AnalyticsQuery>,
) -> Result<Json<UsageStats>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    let api_key_id = query
        .api_key_id
        .ok_or_else(|| ApiError::bad_request("api_key_id query parameter required"))?;

    // Verify ownership
    let api_key = ApiKey::find_by_id(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    if api_key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    // Get current month usage
    let current_month = DailyUsageSummary::get_current_month_total(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(UsageStats {
        api_key_id: api_key.id.to_string(),
        key_prefix: api_key.key_prefix,
        tier: api_key.tier.to_string(),
        monthly_quota: api_key.monthly_message_quota,
        messages_sent_this_month: current_month.total_messages_sent,
        messages_received_this_month: current_month.total_messages_received,
        quota_remaining: api_key.monthly_message_quota as i64 - current_month.total_messages_sent,
        percentage_used: (current_month.total_messages_sent as f64
            / api_key.monthly_message_quota as f64)
            * 100.0,
    }))
}

// Helper structs for SQL queries
#[derive(sqlx::FromRow)]
struct MessageStatsRow {
    total_messages: Option<i64>,
    total_delivered: Option<i64>,
    total_expired: Option<i64>,
    average_access_count: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct ProofStatsRow {
    total_proofs_created: Option<i64>,
    total_proofs_verified: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct UsageStats {
    pub api_key_id: String,
    pub key_prefix: String,
    pub tier: String,
    pub monthly_quota: i32,
    pub messages_sent_this_month: i64,
    pub messages_received_this_month: i64,
    pub quota_remaining: i64,
    pub percentage_used: f64,
}
