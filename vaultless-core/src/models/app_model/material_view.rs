use super::dto::*;
use crate::error::{Result, VaultlessError};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

impl Application {
    pub async fn find_application_by_id<'c, E>(
        exec: E,
        application_id: Uuid,
    ) -> Result<ApplicationWithUsageResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
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
            tier,
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
            quota_usage_percentage,
            lifetime_messages_sent,
            lifetime_messages_received,
            lifetime_proofs_verified,
            lifetime_bytes_stored,
            lifetime_bytes_sent,
            lifetime_bytes_received,
            lifetime_rate_limit_hits,
            lifetime_cost_cents,
            last_7d_messages_sent,
            last_7d_messages_received,
            last_7d_proofs_verified,
            last_7d_bytes_sent,
            last_7d_bytes_received,
            last_7d_cost_cents,
            last_30d_messages_sent,
            last_30d_messages_received,
            last_30d_bytes_sent,
            last_30d_bytes_received,
            last_30d_cost_cents
        FROM public.mv_applications_with_usage
        WHERE application_id = $1
        "#,
        )
        .bind(application_id)
        .fetch_one(exec)
        .await?;

        Ok(app)
    }

    pub async fn list_user_applications<'c, E>(
        exec: E,
        user_id: Uuid,
        page: i64,
        page_size: i64,
    ) -> Result<PaginatedApplicationsWithKeys>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let offset = (page - 1).max(0) * page_size;

        // Query the materialized view directly - much faster!
        let rows: Vec<ApplicationWithKeysFromView> =
            sqlx::query_as::<_, ApplicationWithKeysFromView>(
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
                COUNT(*) OVER() AS total_count
            FROM mv_applications_with_keys
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            )
            .bind(user_id)
            .bind(page_size)
            .bind(offset)
            .fetch_all(exec)
            .await
            .map_err(VaultlessError::Database)?;

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
                internal_notes: r.internal_notes,
                integrity_config: r.integrity_config,
                publishable_keys: r.publishable_keys,
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
}
