use super::dto::*;
use crate::error::{Result, VaultlessError};
use bigdecimal::BigDecimal as Decimal;
use chrono::{DateTime, Utc};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
struct QuotaWarningWithCount {
    pub application_id: Uuid,
    pub application_name: String,
    pub quota_usage_percentage: Decimal,
    pub total_count: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ApplicationSummaryFromView {
    pub application_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tier: Option<String>,
    pub monthly_message_quota: Option<i64>,
    pub publishable_key_count: i64,
    pub webhook_count: i64,
    pub client_count: i64,
    pub quota_usage_percentage: Decimal,
    pub total_count: i64,
}

impl Application {
    /// Find application with complete usage data from the materialized view
    pub async fn find_owned_by_user<'c, E>(
        exec: E,
        application_id: Uuid,
        user_id: Uuid,
    ) -> Result<ApplicationWithUsage>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let app = sqlx::query_as::<_, ApplicationWithUsage>(
            r#"
            SELECT
                application_id, developer_id AS user_id, name, description, is_active,
                created_at, updated_at, app_meta,
                subscription_id, tier::text AS "tier", 
                monthly_message_quota, rate_limit_per_minute, message_retention_seconds,
                secret_key_id, secret_key_prefix,
                publishable_key_count, publishable_keys,
                webhook_count, webhooks,
                client_count,
                current_month_messages_sent, current_month_messages_received,
                current_month_proofs_verified, current_month_bytes_stored,
                current_month_bytes_sent, current_month_bytes_received,
                current_month_rate_limit_hits, current_month_cost_cents,
                quota_usage_percentage,
                lifetime_messages_sent, lifetime_cost_cents
            FROM mv_applications_with_usage
            WHERE application_id = $1 AND developer_id = $2
            "#,
        )
        .bind(application_id)
        .bind(user_id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found or access denied".into()))?;

        Ok(app)
    }

    /// List applications with summary data for pagination
    pub async fn list_user_applications<'c, E>(
        exec: E,
        user_id: Uuid,
        page: i64,
        page_size: i64,
    ) -> Result<PaginatedApplicationsSummary>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let offset = (page - 1).max(0) * page_size;

        let rows = sqlx::query_as::<_, ApplicationSummaryFromView>(
            r#"
            SELECT
                application_id, name, description, is_active,
                created_at, updated_at, tier::text AS "tier",
                monthly_message_quota, publishable_key_count,
                webhook_count, client_count, quota_usage_percentage,
                COUNT(*) OVER() AS total_count
            FROM mv_applications_with_usage
            WHERE developer_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(exec)
        .await?;

        let total_count = rows.first().map(|r| r.total_count).unwrap_or(0);
        let total_pages = (total_count as f64 / page_size as f64).ceil() as i64;

        let data = rows
            .into_iter()
            .map(|r| ApplicationSummary {
                application_id: r.application_id,
                name: r.name,
                description: r.description,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
                tier: r.tier,
                monthly_message_quota: r.monthly_message_quota,
                publishable_key_count: r.publishable_key_count,
                webhook_count: r.webhook_count,
                client_count: r.client_count,
                quota_usage_percentage: r.quota_usage_percentage,
            })
            .collect();

        Ok(PaginatedApplicationsSummary {
            data,
            total_count,
            page,
            page_size,
            total_pages,
        })
    }

    pub async fn get_quota_warnings(
        db: &sqlx::PgPool,
        user_id: Uuid,
        threshold: Option<Decimal>,
        page: i64,
        page_size: i64,
    ) -> Result<PaginatedQuotaWarnings> {
        let threshold = threshold.unwrap_or_else(|| Decimal::from(80));
        let offset = (page - 1).max(0) * page_size;

        let rows = sqlx::query_as::<_, QuotaWarningWithCount>(
            r#"
            SELECT 
                application_id, application_name, quota_usage_percentage,
                COUNT(*) OVER() AS total_count
            FROM get_quota_warnings($1, $2)
            ORDER BY quota_usage_percentage DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(user_id)
        .bind(threshold)
        .bind(page_size)
        .bind(offset)
        .fetch_all(db)
        .await?;

        let total_count = rows.first().map(|r| r.total_count).unwrap_or(0);
        let total_pages = (total_count as f64 / page_size as f64).ceil() as i64;

        let data = rows
            .into_iter()
            .map(|r| QuotaWarning {
                application_id: r.application_id,
                application_name: r.application_name,
                quota_usage_percentage: r.quota_usage_percentage,
            })
            .collect();

        Ok(PaginatedQuotaWarnings {
            data,
            total_count,
            page,
            page_size,
            total_pages,
        })
    }

    pub async fn get_user_usage_summary(
        db: &sqlx::PgPool,
        user_id: Uuid,
    ) -> Result<UserUsageSummary> {
        let summary = sqlx::query_as!(
            UserUsageSummary,
            r#"
        SELECT
            total_apps AS "total_apps!",
            total_monthly_messages AS "total_monthly_messages!",
            total_clients AS "total_clients!",
            total_monthly_cost AS "total_monthly_cost!",
            critical_quota_apps AS "critical_quota_apps!"
        FROM get_user_usage_summary($1)
        "#,
            user_id
        )
        .fetch_one(db)
        .await?;

        Ok(summary)
    }
}
