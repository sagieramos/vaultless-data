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
pub struct MonthlyRevenueDataSchema {
    pub month_label: String,
    pub revenue_cents: i64,
    pub revenue_usd: String,  // Convert BigDecimal to String for schema
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct MonthlyApplicationRevenue {
    pub application_id: Uuid,
    pub application_name: String,
    pub revenue_cents: i64,
}

#[derive(FromRow)]
struct MonthlyApplicationRevenueWithCount {
    application_id: Uuid,
    application_name: String,
    revenue_cents: i64,
    total_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaginatedMonthlyApplicationRevenue {
    pub data: Vec<MonthlyApplicationRevenue>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

impl MonthlyRevenueData {
    /// Get monthly revenue chart data for an application (For Admin use only TODO)
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
        page: i64,
        page_size: i64,
    ) -> Result<PaginatedMonthlyApplicationRevenue>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let offset = (page - 1).max(0) * page_size;
        let rows = sqlx::query_as::<_, MonthlyApplicationRevenueWithCount>(
            r#"
            SELECT
                a.id AS application_id,
                a.name AS application_name,
                COALESCE(SUM(um.estimated_cost_cents), 0) AS revenue_cents,
                COUNT(*) OVER() as total_count
            FROM applications a
            LEFT JOIN usage_metrics um ON a.id = um.application_id
            WHERE a.developer_id = $1
                AND date_trunc('month', um.period_start) = date_trunc('month', $2)
            GROUP BY a.id, a.name
            ORDER BY revenue_cents DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(developer_id)
        .bind(month)
        .bind(page_size)
        .bind(offset)
        .fetch_all(executor)
        .await?;

        let total_count = rows.first().map(|r| r.total_count).unwrap_or(0);
        let total_pages = (total_count as f64 / page_size as f64).ceil() as i64;

        let data = rows
            .into_iter()
            .map(|r| MonthlyApplicationRevenue {
                application_id: r.application_id,
                application_name: r.application_name,
                revenue_cents: r.revenue_cents,
            })
            .collect();

        Ok(PaginatedMonthlyApplicationRevenue {
            data,
            total_count,
            page,
            page_size,
            total_pages,
        })
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