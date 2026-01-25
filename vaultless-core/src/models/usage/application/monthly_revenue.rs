//! Monthly revenue aggregation for charting and analytics
//!
//! Provides efficient queries for monthly revenue data using TimescaleDB continuous aggregates.

use crate::error::Result;
use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal as Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;
use utoipa::ToSchema;

// =============================================================================
// Monthly Revenue Data
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MonthlyRevenueData {
    pub month_label: String,
    pub revenue_cents: i64,
    pub revenue_usd: Decimal,
    pub messages: i64,
    pub bytes_transferred: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RevenueChartData {
    pub labels: Vec<String>,
    pub revenue_cents: Vec<i64>,
    pub messages: Vec<i64>,
    pub bytes_transferred: Vec<i64>,
}

impl MonthlyRevenueData {
    /// Get monthly revenue chart data for an application
    pub async fn get_chart_data_for_application<'c, E>(
        executor: E,
        application_id: Uuid,
        months_back: i32,
    ) -> Result<RevenueChartData>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let rows = sqlx::query_as::<_, MonthlyRevenueData>(
            r#"
            SELECT month_label, revenue_cents, revenue_usd, messages, bytes_transferred
            FROM get_monthly_revenue_chart_data($1, NULL, $2)
            ORDER BY month_label DESC
            "#,
        )
        .bind(application_id)
        .bind(months_back as i32)
        .fetch_all(executor)
        .await?;

        let labels: Vec<String> = rows.iter().map(|r| r.month_label.clone()).collect();
        let revenue_cents: Vec<i64> = rows.iter().map(|r| r.revenue_cents).collect();
        let messages: Vec<i64> = rows.iter().map(|r| r.messages).collect();
        let bytes_transferred: Vec<i64> = rows.iter().map(|r| r.bytes_transferred).collect();

        Ok(RevenueChartData {
            labels,
            revenue_cents,
            messages,
            bytes_transferred,
        })
    }

    /// Get monthly revenue chart data for a developer
    pub async fn get_chart_data_for_developer<'c, E>(
        executor: E,
        developer_id: Uuid,
        months_back: i32,
    ) -> Result<RevenueChartData>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let rows = sqlx::query_as::<_, MonthlyRevenueData>(
            r#"
            SELECT month_label, revenue_cents, revenue_usd, messages, bytes_transferred
            FROM get_monthly_revenue_chart_data(NULL, $1, $2)
            ORDER BY month_label DESC
            "#,
        )
        .bind(developer_id)
        .bind(months_back as i32)
        .fetch_all(executor)
        .await?;

        let labels: Vec<String> = rows.iter().map(|r| r.month_label.clone()).collect();
        let revenue_cents: Vec<i64> = rows.iter().map(|r| r.revenue_cents).collect();
        let messages: Vec<i64> = rows.iter().map(|r| r.messages).collect();
        let bytes_transferred: Vec<i64> = rows.iter().map(|r| r.bytes_transferred).collect();

        Ok(RevenueChartData {
            labels,
            revenue_cents,
            messages,
            bytes_transferred,
        })
    }

    /// Get monthly revenue totals by application for a developer
    pub async fn get_monthly_totals_by_application<'c, E>(
        executor: E,
        developer_id: Uuid,
        month: DateTime<Utc>,
    ) -> Result<Vec<(String, i64)>>  // (app_name, revenue_cents)
    where
        E: Executor<'c, Database = Postgres>,
    {
        let rows = sqlx::query_as::<_, (String, i64)>(
            r#"
            SELECT
                a.name,
                COALESCE(SUM(um.estimated_cost_cents), 0) AS revenue_cents
            FROM applications a
            LEFT JOIN usage_metrics um ON a.id = um.application_id
            WHERE a.developer_id = $1
                AND date_trunc('month', um.period_start) = date_trunc('month', $2)
            GROUP BY a.id, a.name
            ORDER BY revenue_cents DESC
            "#,
        )
        .bind(developer_id)
        .bind(month)
        .fetch_all(executor)
        .await?;

        Ok(rows)
    }

    /// Get monthly revenue chart data for a specific application belonging to a developer
    /// This provides security validation by ensuring the application belongs to the developer
    pub async fn get_chart_data_for_developer_application<'c, E>(
        executor: E,
        developer_id: Uuid,
        application_id: Uuid,
        months_back: i32,
    ) -> Result<RevenueChartData>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let rows = sqlx::query_as::<_, MonthlyRevenueData>(
            r#"
            SELECT month_label, revenue_cents, revenue_usd, messages, bytes_transferred
            FROM get_monthly_revenue_chart_data($1, $2, $3)
            ORDER BY month_label DESC
            "#,
        )
        .bind(application_id)
        .bind(developer_id)
        .bind(months_back as i32)
        .fetch_all(executor)
        .await?;

        let labels: Vec<String> = rows.iter().map(|r| r.month_label.clone()).collect();
        let revenue_cents: Vec<i64> = rows.iter().map(|r| r.revenue_cents).collect();
        let messages: Vec<i64> = rows.iter().map(|r| r.messages).collect();
        let bytes_transferred: Vec<i64> = rows.iter().map(|r| r.bytes_transferred).collect();

        Ok(RevenueChartData {
            labels,
            revenue_cents,
            messages,
            bytes_transferred,
        })
    }
}