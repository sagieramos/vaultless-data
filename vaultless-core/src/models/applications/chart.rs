use super::dto::Application;
use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres};
use std::fmt;
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;

// Define safe maximum data points for each granularity.
// Daily: ~3 months (100 days). Weekly: ~3 years (160 weeks).
const CHART_MAX_POINTS_DAILY: i64 = 100;
const CHART_MAX_POINTS_WEEKLY: i64 = 160;

/// Metric types the frontend can request
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChartMetric {
    Messages,
    Bandwidth,
    Storage,
    Proofs,
    RateLimits,
    All,
}

impl FromStr for ChartMetric {
    type Err = VaultlessError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "messages" => Ok(ChartMetric::Messages),
            "bandwidth" => Ok(ChartMetric::Bandwidth),
            "storage" => Ok(ChartMetric::Storage),
            "proofs" => Ok(ChartMetric::Proofs),
            "rate_limits" => Ok(ChartMetric::RateLimits),
            "all" => Ok(ChartMetric::All),
            _ => Err(VaultlessError::BadRequest(
                "Invalid metric. Must be one of: messages, bandwidth, storage, proofs, rate_limits, all."
                    .into(),
            )),
        }
    }
}

/// Granularity of chart buckets
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChartGranularity {
    Daily,
    Weekly,
}

impl FromStr for ChartGranularity {
    type Err = VaultlessError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "daily" => Ok(ChartGranularity::Daily),
            "weekly" => Ok(ChartGranularity::Weekly),
            _ => Err(VaultlessError::BadRequest(
                "Invalid granularity. Must be 'daily' or 'weekly'.".into(),
            )),
        }
    }
}

impl fmt::Display for ChartGranularity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ChartGranularity::Daily => "daily",
            ChartGranularity::Weekly => "weekly",
        };
        write!(f, "{s}")
    }
}

impl ChartGranularity {
    pub fn bucket_interval(&self) -> &'static str {
        match self {
            ChartGranularity::Daily => "'1 day'::interval",
            ChartGranularity::Weekly => "'1 week'::interval",
        }
    }

    pub fn as_api_str(&self) -> &'static str {
        match self {
            ChartGranularity::Daily => "daily",
            ChartGranularity::Weekly => "weekly",
        }
    }

    pub fn time_column(&self) -> &'static str {
        match self {
            ChartGranularity::Daily => "day",
            ChartGranularity::Weekly => "week_start",
        }
    }
}

/// A single point in a usage chart
#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageChartPoint {
    #[serde(skip_serializing)]
    pub application_name: String,

    #[schema(example = "2023-01-01T00:00:00Z")]
    pub timestamp: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 100)]
    pub messages_sent: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 50)]
    pub messages_received: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 25)]
    pub proofs_verified: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 102400)]
    pub bytes_sent: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 51200)]
    pub bytes_received: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 204800)]
    pub bytes_stored: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 5)]
    pub rate_limit_hits: Option<i64>,
}

/// Aggregated chart data for a single application
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationChartData {
    #[schema(value_type = String)]
    pub application_id: Uuid,
    pub application_name: String,
    pub time_range: String,
    pub granularity: String,
    pub metric_view: ChartMetric,
    pub data_points: Vec<UsageChartPoint>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_messages_sent: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_messages_received: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_proofs_verified: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes_sent: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes_received: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes_stored: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_rate_limit_hits: Option<i64>,

    /// Optional trend data comparing current period vs previous period
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trends: Option<ChartTrends>,
}

/// Trend data comparing current period vs previous period
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChartTrends {
    /// Current period total (messages or bytes depending on metric)
    pub current_period: i64,
    /// Previous period total
    pub previous_period: i64,
    /// Percentage change (-100 to +N)
    pub change_percent: f64,
    /// "up", "down", or "stable"
    pub trend_direction: String,
}

impl Application {
    /// Fetch chart data for a given app, metric, and bucket range in a single query.
    pub async fn get_chart_data(
        pool: &sqlx::PgPool,
        app_id: Uuid,
        user_id: Uuid,
        granularity: ChartGranularity,
        metric_view: ChartMetric,
        start_ts: DateTime<Utc>,
        end_ts: DateTime<Utc>,
        include_trends: bool,
    ) -> Result<ApplicationChartData> {
        let table_name = match granularity {
            ChartGranularity::Daily => "application_usage_metrics_daily",
            ChartGranularity::Weekly => "application_usage_metrics_weekly",
        };

        let bucket_interval = granularity.bucket_interval();
        let time_column = granularity.time_column();

        let max_points = match granularity {
            ChartGranularity::Daily => CHART_MAX_POINTS_DAILY,
            ChartGranularity::Weekly => CHART_MAX_POINTS_WEEKLY,
        };

        let sql = format!(
            r#"
            WITH time_series AS (
                SELECT generate_series(
                    $1,
                    $2,
                    {bucket_interval}::interval
                ) AS timestamp
            )
            SELECT
                a.name AS application_name,
                ts.timestamp,
                COALESCE(m.total_messages_sent, 0) AS messages_sent,
                COALESCE(m.total_messages_received, 0) AS messages_received,
                COALESCE(m.total_proofs_verified, 0) AS proofs_verified,
                COALESCE(m.total_bytes_sent, 0) AS bytes_sent,
                COALESCE(m.total_bytes_received, 0) AS bytes_received,
                COALESCE(m.total_bytes_stored, 0) AS bytes_stored,
                COALESCE(m.total_rate_limit_hits, 0) AS rate_limit_hits
            FROM applications a
            INNER JOIN api_keys k ON k.application_id = a.id
                AND k.key_type = 'secret'
                AND k.is_active = true
            CROSS JOIN time_series ts
            LEFT JOIN {table_name} m ON m.application_id = a.id
                AND m.{time_column} = ts.timestamp
            WHERE a.id = $3
              AND a.developer_id = $4
              AND a.is_active = true
            ORDER BY ts.timestamp ASC
            LIMIT $5
            "#
        );

        let rows = sqlx::query_as::<_, UsageChartPoint>(&sql)
            .bind(start_ts)
            .bind(end_ts)
            .bind(app_id)
            .bind(user_id)
            .bind(max_points)
            .fetch_all(pool)
            .await?;

        if rows.is_empty() {
            return Err(VaultlessError::NotFound(
                "Application not found, you don't have permission, or no secret key exists".into(),
            ));
        }

        let app_name = rows[0].application_name.clone();

        // Sum all data points for totals (values are already COALESCE'd to 0)
        let sum_values = |f: fn(&UsageChartPoint) -> Option<i64>| -> Option<i64> {
            Some(rows.iter().filter_map(f).sum())
        };

        // Calculate trends if requested using PostgreSQL function
        let trends = if include_trends {
            calculate_trends_db(
                pool,
                app_id,
                start_ts,
                end_ts,
                granularity,
                metric_view,
            )
            .await
            .ok()
        } else {
            None
        };

        Ok(ApplicationChartData {
            application_id: app_id,
            application_name: app_name,
            metric_view,
            time_range: format!(
                "{} to {}",
                start_ts.format("%Y-%m-%d"),
                end_ts.format("%Y-%m-%d")
            ),
            granularity: granularity.as_api_str().to_owned(),

            total_messages_sent: sum_values(|p| p.messages_sent),
            total_messages_received: sum_values(|p| p.messages_received),
            total_proofs_verified: sum_values(|p| p.proofs_verified),
            total_bytes_sent: sum_values(|p| p.bytes_sent),
            total_bytes_received: sum_values(|p| p.bytes_received),
            total_bytes_stored: sum_values(|p| p.bytes_stored),
            total_rate_limit_hits: sum_values(|p| p.rate_limit_hits),

            data_points: rows,
            trends,
        })
    }
}

/// Calculate trends by calling PostgreSQL function
async fn calculate_trends_db(
    pool: &sqlx::PgPool,
    app_id: Uuid,
    start_ts: DateTime<Utc>,
    end_ts: DateTime<Utc>,
    granularity: ChartGranularity,
    metric_view: ChartMetric,
) -> Result<ChartTrends> {
    let granularity_str = granularity.as_api_str();
    let metric_str = match metric_view {
        ChartMetric::Messages => "messages",
        ChartMetric::Bandwidth => "bandwidth",
        ChartMetric::Storage => "storage",
        ChartMetric::Proofs => "proofs",
        ChartMetric::RateLimits => "rate_limits",
        ChartMetric::All => "messages",
    };

    let result: (i64, i64, f64, String) = sqlx::query_as(
        r#"
        SELECT 
            current_period,
            previous_period,
            change_percent,
            trend_direction
        FROM calculate_chart_trends(
            $1,
            $2,
            $3,
            $4,
            $5
        )
        "#
    )
    .bind(app_id)
    .bind(start_ts)
    .bind(end_ts)
    .bind(granularity_str)
    .bind(metric_str)
    .fetch_one(pool)
    .await?;

    Ok(ChartTrends {
        current_period: result.0,
        previous_period: result.1,
        change_percent: result.2,
        trend_direction: result.3,
    })
}
