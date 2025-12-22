use super::dto::Application;
use utoipa::ToSchema;
use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use std::fmt;
use std::str::FromStr;
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
    Cost,
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
            "cost" => Ok(ChartMetric::Cost),
            "all" => Ok(ChartMetric::All),
            _ => Err(VaultlessError::BadRequest(
                "Invalid metric. Must be one of: messages, bandwidth, storage, proofs, rate_limits, cost, all.".into(),
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
        write!(f, "{}", s)
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

    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 500)]
    pub cost_cents: Option<i64>,
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
    #[schema(example = 1000)]
    pub total_messages_sent: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 500)]
    pub total_messages_received: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 250)]
    pub total_proofs_verified: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 1024000)]
    pub total_bytes_sent: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 512000)]
    pub total_bytes_received: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 2048000)]
    pub total_bytes_stored: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 50)]
    pub total_rate_limit_hits: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = 5000)]
    pub total_cost_cents: Option<i64>,
}

impl Application {
    /// Fetch chart data for a given app, metric, and bucket range in a single query.
    ///
    /// The query performs security validation, fetches data, gapfills the time series,
    /// and ensures the data points are within a safe LIMIT.
    pub async fn get_chart_data<'c, E>(
        exec: E,
        app_id: Uuid,
        user_id: Uuid,
        granularity: ChartGranularity,
        metric_view: ChartMetric,
        start_ts: DateTime<Utc>,
        end_ts: DateTime<Utc>,
    ) -> Result<ApplicationChartData>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let table_name = match granularity {
            ChartGranularity::Daily => "usage_metrics_daily",
            ChartGranularity::Weekly => "usage_metrics_weekly",
        };
        let bucket_interval = granularity.bucket_interval();
        let time_column = granularity.time_column();

        // Determine the safe maximum points based on granularity
        let max_points = match granularity {
            ChartGranularity::Daily => CHART_MAX_POINTS_DAILY,
            ChartGranularity::Weekly => CHART_MAX_POINTS_WEEKLY,
        };

        // NOTE: Must use MAX() for TimescaleDB Continuous Aggregates to fetch the pre-calculated total.
        let select_fields = match metric_view {
            ChartMetric::Messages => {
                "COALESCE(MAX(m.total_messages_sent), 0) AS messages_sent,
                 COALESCE(MAX(m.total_messages_received), 0) AS messages_received"
            }
            ChartMetric::Bandwidth => {
                "COALESCE(MAX(m.total_bytes_sent), 0) AS bytes_sent,
                 COALESCE(MAX(m.total_bytes_received), 0) AS bytes_received"
            }
            ChartMetric::Storage => "COALESCE(MAX(m.total_bytes_stored), 0) AS bytes_stored",
            ChartMetric::Proofs => "COALESCE(MAX(m.total_proofs_verified), 0) AS proofs_verified",
            ChartMetric::RateLimits => {
                "COALESCE(MAX(m.total_rate_limit_hits), 0) AS rate_limit_hits"
            }
            ChartMetric::Cost => "COALESCE(MAX(m.total_estimated_cost_cents), 0) AS cost_cents",
            ChartMetric::All => {
                "COALESCE(MAX(m.total_messages_sent), 0) AS messages_sent,
                 COALESCE(MAX(m.total_messages_received), 0) AS messages_received,
                 COALESCE(MAX(m.total_proofs_verified), 0) AS proofs_verified,
                 COALESCE(MAX(m.total_bytes_sent), 0) AS bytes_sent,
                 COALESCE(MAX(m.total_bytes_received), 0) AS bytes_received,
                 COALESCE(MAX(m.total_bytes_stored), 0) AS bytes_stored,
                 COALESCE(MAX(m.total_rate_limit_hits), 0) AS rate_limit_hits,
                 COALESCE(MAX(m.total_estimated_cost_cents), 0) AS cost_cents"
            }
        };

        // $1: start_ts, $2: end_ts, $3: app_id, $4: user_id, $5: limit
        let sql = format!(
            r#"
            SELECT
                a.name AS application_name,
                -- Time bucket gapfill ensures a continuous series between $1 and $2
                time_bucket_gapfill({bucket_interval}, m.{time_column}, $1, $2) AS timestamp,
                {select_fields}
            FROM applications a
            INNER JOIN api_keys k ON k.application_id = a.id
                AND k.key_type = 'secret'
                AND k.is_active = true
            -- LEFT JOIN to metrics view to allow for applications with no usage data
            LEFT JOIN {table_name} m ON m.application_id = a.id
                -- Range Bounding: Only join metrics within the time range
                AND m.{time_column} >= $1
                AND m.{time_column} <= $2
            WHERE a.id = $3 AND a.user_id = $4 AND a.is_active = true
            GROUP BY a.name, timestamp
            ORDER BY timestamp ASC
            LIMIT $5
            "#,
            bucket_interval = bucket_interval,
            time_column = time_column,
            table_name = table_name,
            select_fields = select_fields,
        );

        let rows = sqlx::query_as::<_, UsageChartPoint>(&sql)
            .bind(start_ts) // $1
            .bind(end_ts) // $2
            .bind(app_id) // $3
            .bind(user_id) // $4
            .bind(max_points) // $5: Apply the dynamic limit
            .fetch_all(exec)
            .await?;

        // If no rows are returned, the application either doesn't exist, the user
        // doesn't own it, or it has no active secret key.
        if rows.is_empty() {
            return Err(VaultlessError::NotFound(
                "Application not found, you don't have permission, or no secret key exists".into(),
            ));
        }

        let app_name = rows[0].application_name.clone();

        // Helper to sum nullable fields and correctly handle unselected metrics.
        let sum_or_none = |f: fn(&UsageChartPoint) -> Option<i64>| -> Option<i64> {
            // Check if the column was selected in the SQL (i.e., if it has a value in the first row).
            // If the column was NOT selected, sqlx sets it to None, and we return None here.
            rows.first().and_then(f)?;
            // If it was selected, sum all values (0s from COALESCE/gapfill will be summed correctly).
            Some(rows.iter().filter_map(f).sum())
        };

        Ok(ApplicationChartData {
            application_id: app_id,
            application_name: app_name,
            metric_view,
            time_range: format!(
                "{} → {}",
                start_ts.format("%Y-%m-%d"),
                end_ts.format("%Y-%m-%d")
            ),
            granularity: granularity.as_api_str().to_owned(),

            // Totals: These correctly return None for metrics not included in the SELECT statement
            // due to the check inside sum_or_none.
            total_messages_sent: sum_or_none(|p| p.messages_sent),
            total_messages_received: sum_or_none(|p| p.messages_received),
            total_proofs_verified: sum_or_none(|p| p.proofs_verified),
            total_bytes_sent: sum_or_none(|p| p.bytes_sent),
            total_bytes_received: sum_or_none(|p| p.bytes_received),
            total_bytes_stored: sum_or_none(|p| p.bytes_stored),
            total_rate_limit_hits: sum_or_none(|p| p.rate_limit_hits),
            total_cost_cents: sum_or_none(|p| p.cost_cents),

            data_points: rows,
        })
    }
}
