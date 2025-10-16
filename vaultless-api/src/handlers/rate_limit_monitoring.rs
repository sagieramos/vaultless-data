use axum::{Extension, Json, extract::State};
use serde::Serialize;
use vaultless_core::ApiKey;

use crate::{middleware::error::ApiError, services::RateLimiter, state::AppState};

/// Rate limit monitoring for authenticated user
#[derive(Debug, Serialize)]
pub struct RateLimitMonitoring {
    pub current_limit: RateLimitInfo,
    pub violations: ViolationInfo,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RateLimitInfo {
    pub requests_per_minute: i32,
    pub current_usage: i64,
    pub remaining: i64,
    pub usage_percentage: f64,
    pub window_start: u64,
    pub window_end: u64,
}

#[derive(Debug, Serialize)]
pub struct ViolationInfo {
    pub count_24h: i64,
    pub severity: String, // "low", "medium", "high"
    pub warning: Option<String>,
}

/// Get rate limit monitoring for current API key
/// GET /api/v1/rate-limit/status
pub async fn get_my_rate_limit_status(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
) -> Result<Json<RateLimitMonitoring>, ApiError> {
    let rate_limiter = RateLimiter::new(state.cache.clone());

    // Get current usage
    let usage = rate_limiter.get_current_usage(api_key.id).await?;
    let violations = rate_limiter.get_violation_count(api_key.id).await?;

    let remaining = api_key.rate_limit_per_minute as i64 - usage.requests_in_window;
    let usage_percentage =
        (usage.requests_in_window as f64 / api_key.rate_limit_per_minute as f64) * 100.0;

    // Determine severity
    let (severity, warning) = if violations > 100 {
        (
            "high",
            Some("Your API key has been rate limited frequently. Consider upgrading your plan."),
        )
    } else if violations > 10 {
        (
            "medium",
            Some("You're approaching rate limits often. Monitor your usage."),
        )
    } else {
        ("low", None)
    };

    // Generate recommendations
    let mut recommendations = Vec::new();

    if usage_percentage > 80.0 {
        recommendations.push(
            "You're using over 80% of your rate limit. Consider implementing request batching."
                .to_string(),
        );
    }

    if violations > 0 {
        recommendations
            .push("Implement exponential backoff when you receive 429 errors.".to_string());
    }

    if api_key.rate_limit_per_minute < 1000 {
        recommendations.push(format!(
            "Upgrade to a higher tier for more requests. Current limit: {} req/min.",
            api_key.rate_limit_per_minute
        ));
    }

    if recommendations.is_empty() {
        recommendations.push("Your rate limit usage looks healthy!".to_string());
    }

    Ok(Json(RateLimitMonitoring {
        current_limit: RateLimitInfo {
            requests_per_minute: api_key.rate_limit_per_minute,
            current_usage: usage.requests_in_window,
            remaining,
            usage_percentage,
            window_start: usage.window_start,
            window_end: usage.window_end,
        },
        violations: ViolationInfo {
            count_24h: violations,
            severity: severity.to_string(),
            warning: warning.map(String::from),
        },
        recommendations,
    }))
}

/// Get rate limit history (last 24 hours)
/// GET /api/v1/rate-limit/history
pub async fn get_rate_limit_history(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
) -> Result<Json<RateLimitHistory>, ApiError> {
    // Query usage metrics for last 24 hours
    let history = sqlx::query_as::<_, HourlyUsage>(
        r#"
        SELECT 
            DATE_TRUNC('hour', period_start) as hour,
            SUM(messages_sent + messages_received) as total_requests,
            SUM(rate_limit_hits) as rate_limit_hits
        FROM usage_metrics
        WHERE api_key_id = $1
            AND period_start > NOW() - INTERVAL '24 hours'
        GROUP BY DATE_TRUNC('hour', period_start)
        ORDER BY hour DESC
        "#,
    )
    .bind(api_key.id)
    .fetch_all(&state.db)
    .await
    .map_err(ApiError::from)?;

    let total_requests: i64 = history.iter().map(|h| h.total_requests.unwrap_or(0)).sum();
    let total_violations: i64 = history.iter().map(|h| h.rate_limit_hits.unwrap_or(0)).sum();
    let hours_tracked = history.len();

    Ok(Json(RateLimitHistory {
        hourly_data: history,
        summary: HistorySummary {
            total_requests,
            total_violations,
            hours_tracked,
        },
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct HourlyUsage {
    pub hour: chrono::DateTime<chrono::Utc>,
    pub total_requests: Option<i64>,
    pub rate_limit_hits: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct RateLimitHistory {
    pub hourly_data: Vec<HourlyUsage>,
    pub summary: HistorySummary,
}

#[derive(Debug, Serialize)]
pub struct HistorySummary {
    pub total_requests: i64,
    pub total_violations: i64,
    pub hours_tracked: usize,
}
