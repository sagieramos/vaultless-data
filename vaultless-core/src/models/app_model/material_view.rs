use super::dto::*;
use crate::error::{Result, VaultlessError};
use bigdecimal::BigDecimal as Decimal;
use sqlx::{Executor, Postgres};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct QuotaWarningWithCount {
    pub application_id: Option<Uuid>,
    pub application_name: Option<String>,
    pub quota_usage_percentage: Option<Decimal>,
    pub current_month_messages_sent: Option<i64>,
    pub monthly_message_quota: Option<i64>,
    pub remaining_quota: Option<i64>,
    pub total_count: Option<i64>,
}

impl Application {
    /// Find application with complete usage data
    pub async fn find_owned_by_user<'c, E>(
        exec: E,
        application_id: Uuid,
        user_id: Uuid,
    ) -> Result<ApplicationWithUsageResponse>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let app = sqlx::query_as::<_, ApplicationWithUsageResponse>(
            r#"
            SELECT
                application_id,
                name,
                description,
                is_active,
                created_at,
                updated_at,
                max_ttl_seconds,
                is_key_rotation_forced,
                deletion_requested_at,
                integrity_config,
                tier::text as "tier",
                monthly_message_quota,
                rate_limit_per_minute,
                message_retention_seconds,
                publishable_keys,
                webhooks,
                current_month_messages_sent,
                current_month_messages_received,
                current_month_proofs_verified,
                current_month_bytes_stored,
                current_month_bytes_sent,
                current_month_bytes_received,
                current_month_rate_limit_hits,
                current_month_cost_cents,
                CAST(quota_usage_percentage AS DOUBLE PRECISION) as "quota_usage_percentage",
                lifetime_messages_sent,
                lifetime_messages_received,
                lifetime_proofs_verified,
                lifetime_bytes_stored,
                lifetime_bytes_sent,
                lifetime_bytes_received,
                lifetime_rate_limit_hits,
                lifetime_cost_cents,
                last_7d_messages_sent,
                last_7d_bytes_sent,
                last_7d_bytes_received,
                last_7d_cost_cents,
                last_30d_messages_sent,
                last_30d_bytes_sent,
                last_30d_bytes_received,
                last_30d_cost_cents
            FROM mv_applications_with_usage
            WHERE application_id = $1 AND user_id = $2
            "#,
        )
        .bind(application_id)
        .bind(user_id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Application not found or you don't have permission".into())
        })?;

        Ok(app)
    }

    /// List applications (simpler, without usage details)
    pub async fn list_user_applications<'c, E>(
        exec: E,
        user_id: Uuid,
        page: i64,
        page_size: i64,
    ) -> Result<PaginatedApplicationsWithKeys>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let offset = (page - 1).max(0) * page_size;

        let rows = sqlx::query_as::<_, ApplicationWithKeysFromView>(
            r#"
            SELECT 
                application_id,
                user_id,
                name,
                description,
                is_active,
                created_at,
                updated_at,
                max_ttl_seconds,
                is_key_rotation_forced,
                deletion_requested_at,
                integrity_config,
                publishable_keys,
                publishable_key_count,
                webhooks,
                webhook_count,
                COUNT(*) OVER() AS total_count
            FROM mv_applications_with_usage
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(exec)
        .await?;

        if rows.is_empty() {
            return Ok(PaginatedApplicationsWithKeys {
                data: vec![],
                total_count: 0,
                page,
                page_size,
                total_pages: 0,
            });
        }

        let total_count = rows[0].total_count;
        let total_pages = (total_count as f64 / page_size as f64).ceil() as i64;

        let data = rows
            .into_iter()
            .map(|r| ApplicationWithKeysResponse {
                id: r.application_id,
                name: r.name,
                description: r.description,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
                max_ttl_seconds: r.max_ttl_seconds,
                is_key_rotation_forced: r.is_key_rotation_forced,
                deletion_requested_at: r.deletion_requested_at,
                internal_notes: None,
                integrity_config: r.integrity_config,
                publishable_keys: r.publishable_keys,
                webhooks: r.webhooks,
            })
            .collect();

        Ok(PaginatedApplicationsWithKeys {
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
    ) -> sqlx::Result<PaginatedQuotaWarnings> {
        let threshold = threshold.unwrap_or(Decimal::from(80));
        let offset = (page - 1).max(0) * page_size;

        // Query with window function to get total_count
        let rows = sqlx::query_as!(
            QuotaWarningWithCount,
            r#"
            SELECT *
            FROM (
                SELECT 
                    application_id,
                    application_name,
                    quota_usage_percentage,
                    current_month_messages_sent,
                    monthly_message_quota,
                    remaining_quota,
                    COUNT(*) OVER() AS total_count
                FROM get_quota_warnings($1, $2)
            ) t
            ORDER BY quota_usage_percentage DESC
            LIMIT $3 OFFSET $4
            "#,
            user_id,
            threshold,
            page_size,
            offset
        )
        .fetch_all(db)
        .await?;

        let total_count: i64 = rows.first().and_then(|r| r.total_count).unwrap_or(0);
        let total_pages = (total_count as f64 / page_size as f64).ceil() as i64;

        let data = rows
            .into_iter()
            .map(|r| QuotaWarning {
                application_id: r.application_id,
                application_name: r.application_name,
                quota_usage_percentage: r.quota_usage_percentage,
                current_month_messages_sent: r.current_month_messages_sent,
                monthly_message_quota: r.monthly_message_quota,
                remaining_quota: r.remaining_quota,
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
    ) -> sqlx::Result<UserUsageSummary> {
        sqlx::query_as!(
            UserUsageSummary,
            r#"
        SELECT
            total_applications,
            active_applications,
            total_messages_sent_current_month,
            total_messages_received_current_month,
            total_cost_cents_current_month,
            total_lifetime_messages,
            total_lifetime_cost_cents,
            apps_over_80_percent_quota,
            apps_over_quota
        FROM get_user_usage_summary($1)
        "#,
            user_id
        )
        .fetch_one(db)
        .await
    }

    pub async fn get_application_with_keys(
        db: &sqlx::PgPool,
        application_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<ApplicationWithKeys> {
        sqlx::query_as::<_, ApplicationWithKeys>(
            r#"
            SELECT
                application_id,
                user_id,
                name,
                description,
                is_active,
                created_at,
                updated_at,
                max_ttl_seconds,
                is_key_rotation_forced,
                deletion_requested_at,
                integrity_config,
                publishable_keys,
                webhooks,
                secret_key_id
            FROM mv_applications_with_usage
            WHERE application_id = $1 AND user_id = $2
            "#,
        )
        .bind(application_id)
        .bind(user_id)
        .fetch_one(db)
        .await
    }
}
