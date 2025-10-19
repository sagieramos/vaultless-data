use std::f32::consts::E;

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vaultless_core::ApiKey;

use crate::{
    middleware::error::ApiError,
    services::{RateLimiter, token::SessionData},
    state::AppState,
};

// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================

#[derive(Debug, Serialize)]
pub struct RateLimitMonitoring {
    pub api_key_id: String,
    pub key_prefix: String,
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

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct HourlyUsage {
    pub hour: chrono::DateTime<chrono::Utc>,
    pub total_requests: Option<i64>,
    pub rate_limit_hits: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct RateLimitHistory {
    pub api_key_id: String,
    pub key_prefix: String,
    pub hourly_data: Vec<HourlyUsage>,
    pub summary: HistorySummary,
}

#[derive(Debug, Serialize)]
pub struct HistorySummary {
    pub total_requests: i64,
    pub total_violations: i64,
    pub hours_tracked: usize,
}

#[derive(Debug, Deserialize)]
pub struct RateLimitRequest {
    pub api_key_id: Option<Uuid>,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// Fetch rate limit status for user's API key(s)
/// POST /api/v1/rate-limit/status

pub async fn fetch_rate_limit_status(
    State(state): State<AppState>,
    session: SessionData,
    Json(payload): Json<RateLimitRequest>,
) -> Result<Json<Vec<RateLimitMonitoring>>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    tracing::debug!(user_id = %user_id, "Fetching rate limit status");

    // Get user's API keys
    let api_keys = if let Some(key_id) = payload.api_key_id {
        let key = ApiKey::find_by_id(&state.db, key_id)
            .await
            .map_err(ApiError::from)?;
        if key.user_id != user_id {
            return Err(ApiError::forbidden("You don't own this API key"));
        }
        vec![key]
    } else {
        ApiKey::find_by_owner(&state.db, user_id)
            .await
            .map_err(ApiError::from)?
    };

    let rate_limiter = RateLimiter::new(state.cache.clone());
    let mut results = Vec::new();

    for api_key in api_keys {
        let usage = rate_limiter
            .get_current_usage(api_key.id)
            .await
            .unwrap_or_default();
        let violations = rate_limiter
            .get_violation_count(api_key.id)
            .await
            .unwrap_or(0);

        let remaining = api_key.rate_limit_per_minute as i64 - usage.requests_in_window;
        let usage_percentage =
            (usage.requests_in_window as f64 / api_key.rate_limit_per_minute as f64) * 100.0;

        let (severity, warning) = if violations > 100 {
            (
                "high",
                Some(
                    "Your API key has been rate limited frequently. Consider upgrading your plan.",
                ),
            )
        } else if violations > 10 {
            (
                "medium",
                Some("You're approaching rate limits often. Monitor your usage."),
            )
        } else {
            ("low", None)
        };

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

        results.push(RateLimitMonitoring {
            api_key_id: api_key.id.to_string(),
            key_prefix: api_key.key_prefix,
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
        });
    }

    Ok(Json(results))
}

/// Fetch rate limit history (last 24 hours) for user's API key
/// POST /api/v1/rate-limit/history
pub async fn fetch_rate_limit_history(
    State(state): State<AppState>,
    session: SessionData,
    Json(payload): Json<RateLimitRequest>,
) -> Result<Json<Vec<RateLimitHistory>>, ApiError> {
    let api_key_id = payload
        .api_key_id
        .ok_or_else(|| ApiError::bad_request("api_key_id is required"))?;

    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    if api_key_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    // Verify ownership
    let api_key = ApiKey::find_by_id(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    tracing::debug!(api_key_id = %api_key_id, "Fetching rate limit history");

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
    .bind(api_key_id)
    .fetch_all(&state.db)
    .await
    .map_err(ApiError::from)?;

    let total_requests: i64 = history.iter().map(|h| h.total_requests.unwrap_or(0)).sum();
    let total_violations: i64 = history.iter().map(|h| h.rate_limit_hits.unwrap_or(0)).sum();
    let hours_tracked = history.len();

    Ok(Json(vec![RateLimitHistory {
        api_key_id: api_key.id.to_string(),
        key_prefix: api_key.key_prefix,
        hourly_data: history,
        summary: HistorySummary {
            total_requests,
            total_violations,
            hours_tracked,
        },
    }]))
}

/// Get rate limit summary for all user's API keys
/// GET /api/v1/rate-limit/summary
pub async fn get_rate_limit_summary(
    State(state): State<AppState>,
    session: SessionData,
) -> Result<Json<RateLimitSummary>, ApiError> {
    // Get all user's API keys

    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    let api_keys = ApiKey::find_by_owner(&state.db, user_id)
        .await
        .map_err(ApiError::from)?;

    let rate_limiter = RateLimiter::new(state.cache.clone());

    let mut total_violations = 0i64;
    let mut total_capacity = 0i32;
    let mut total_usage = 0i64;
    let mut keys_at_risk = 0;

    for key in &api_keys {
        let violations = rate_limiter.get_violation_count(key.id).await.unwrap_or(0);
        let usage = rate_limiter.get_current_usage(key.id).await.ok();

        total_violations += violations;
        total_capacity += key.rate_limit_per_minute;

        if let Some(u) = usage {
            total_usage += u.requests_in_window;
            let usage_pct =
                (u.requests_in_window as f64 / key.rate_limit_per_minute as f64) * 100.0;
            if usage_pct > 80.0 {
                keys_at_risk += 1;
            }
        }
    }

    Ok(Json(RateLimitSummary {
        total_api_keys: api_keys.len(),
        total_capacity,
        total_usage,
        total_violations_24h: total_violations,
        keys_at_risk,
        health_status: if keys_at_risk > 0 {
            "warning"
        } else if total_violations > 0 {
            "caution"
        } else {
            "healthy"
        }
        .to_string(),
    }))
}

#[derive(Debug, Serialize)]
pub struct RateLimitSummary {
    pub total_api_keys: usize,
    pub total_capacity: i32,
    pub total_usage: i64,
    pub total_violations_24h: i64,
    pub keys_at_risk: usize,
    pub health_status: String,
}
