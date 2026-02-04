use super::dto::{QuotaType, *};
use crate::error::{Result, VaultlessError};
use crate::models::pricing::enums::PricingMode;
use bigdecimal::BigDecimal as Decimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, Postgres};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
struct QuotaWarningWithCount {
    pub application_id: Uuid,
    pub application_name: String,
    pub quota_usage_percentage: Decimal,
    pub bandwidth_quota_usage_percentage: Decimal,
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
    pub bandwidth_quota_usage_percentage: Decimal,
    pub current_month_revenue_cents: i64,
    pub billable_clients_count: i32,
    pub total_count: i64,
}

/// Pricing plan attached to an application
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachedPricingPlan {
    pub id: Uuid,
    pub name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub is_default: bool,
    pub attached_at: DateTime<Utc>,
}

/// Application with usage data and attached pricing plan
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationWithUsageAndPricingPlan {
    #[serde(flatten)]
    pub application: ApplicationWithUsage,
    pub pricing_plan: Option<AttachedPricingPlan>,
}

/// Internal struct for DB row mapping
#[derive(Debug, Clone, sqlx::FromRow)]
struct ApplicationWithPricingPlanRow {
    // Application fields (from mv.*)
    pub application_id: Uuid,
    pub developer_id: Uuid,  // Note: maps to user_id in ApplicationWithUsage
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub app_meta: sqlx::types::Json<super::integrity::dto::AppMetaData>,
    pub subscription_id: Option<Uuid>,
    pub tier: Option<String>,
    pub monthly_message_quota: Option<i64>,
    pub rate_limit_per_minute: Option<i32>,
    pub message_retention_seconds: Option<i64>,
    pub secret_key_id: Option<Uuid>,
    pub secret_key_prefix: Option<String>,
    pub publishable_key_count: i64,
    pub publishable_keys: sqlx::types::Json<Vec<PublishableKey>>,
    pub webhook_count: i64,
    pub webhooks: sqlx::types::Json<Vec<Webhook>>,
    pub client_count: i64,
    pub current_month_messages_sent: i64,
    pub current_month_messages_received: i64,
    pub current_month_proofs_verified: i64,
    pub current_month_bytes_stored: i64,
    pub current_month_bytes_sent: i64,
    pub current_month_bytes_received: i64,
    pub current_month_rate_limit_hits: i64,
    pub quota_usage_percentage: Decimal,
    pub bandwidth_quota_usage_percentage: Decimal,
    pub current_month_revenue_cents: i64,
    pub billable_clients_count: i32,
    pub lifetime_messages_sent: i64,
    // Pricing plan fields (nullable)
    pub pricing_plan_id: Option<Uuid>,
    pub pricing_plan_name: Option<String>,
    pub pricing_mode: Option<PricingMode>,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub pricing_plan_is_default: Option<bool>,
    pub pricing_plan_attached_at: Option<DateTime<Utc>>,
}

impl Application {
    /// Find application with complete usage data from the materialized view.
    /// Optionally includes attached pricing plan when `include_pricing_plan` is true.
    pub async fn find_owned_by_user<'c, E>(
        exec: E,
        application_id: Uuid,
        user_id: Uuid,
        include_pricing_plan: bool,
    ) -> Result<ApplicationWithUsageAndPricingPlan>
    where
        E: Executor<'c, Database = Postgres>,
    {
        if !include_pricing_plan {
            let row = sqlx::query_as::<_, ApplicationWithUsage>(
                r#"
            SELECT
                application_id,
                developer_id AS user_id,
                name,
                description,
                is_active,
                created_at,
                updated_at,
                app_meta,
                subscription_id,
                tier::text AS tier,
                monthly_message_quota,
                rate_limit_per_minute,
                message_retention_seconds,
                secret_key_id,
                secret_key_prefix,
                publishable_key_count,
                publishable_keys,
                webhook_count,
                webhooks,
                client_count,
                current_month_messages_sent,
                current_month_messages_received,
                current_month_proofs_verified,
                current_month_bytes_stored,
                current_month_bytes_sent,
                current_month_bytes_received,
                current_month_rate_limit_hits,
                quota_usage_percentage,
                bandwidth_quota_usage_percentage,
                current_month_revenue_cents,
                billable_clients_count,
                lifetime_messages_sent,
            FROM mv_applications_with_usage
            WHERE application_id = $1 AND developer_id = $2
            "#,
            )
            .bind(application_id)
            .bind(user_id)
            .fetch_optional(exec)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

            return Ok(ApplicationWithUsageAndPricingPlan {
                application: row.into(),
                pricing_plan: None,
            });
        }

        let row = sqlx::query_as::<_, ApplicationWithPricingPlanRow>(
            r#"
        SELECT
            mv.application_id,
            mv.developer_id,
            mv.name,
            mv.description,
            mv.is_active,
            mv.created_at,
            mv.updated_at,
            mv.app_meta,
            mv.subscription_id,
            mv.tier::text AS tier,
            mv.monthly_message_quota,
            mv.rate_limit_per_minute,
            mv.message_retention_seconds,
            mv.secret_key_id,
            mv.secret_key_prefix,
            mv.publishable_key_count,
            mv.publishable_keys,
            mv.webhook_count,
            mv.webhooks,
            mv.client_count,
            mv.current_month_messages_sent,
            mv.current_month_messages_received,
            mv.current_month_proofs_verified,
            mv.current_month_bytes_stored,
            mv.current_month_bytes_sent,
            mv.current_month_bytes_received,
            mv.current_month_rate_limit_hits,
            mv.quota_usage_percentage,
            mv.bandwidth_quota_usage_percentage,
            mv.current_month_revenue_cents,
            mv.billable_clients_count,
            mv.lifetime_messages_sent,
            -- Pricing plan fields
            pp.id AS pricing_plan_id,
            pp.name AS pricing_plan_name,
            pp.pricing_mode,
            pp.price_per_message_cents,
            pp.price_per_gb_cents,
            pp.price_per_proof_cents,
            pp.prepaid_amount_cents,
            app.is_default AS pricing_plan_is_default,
            app.attached_at AS pricing_plan_attached_at
        FROM mv_applications_with_usage mv
        LEFT JOIN application_pricing_plans app
            ON mv.application_id = app.application_id
        LEFT JOIN pricing_plans pp
            ON app.pricing_plan_id = pp.id
        WHERE mv.application_id = $1 AND mv.developer_id = $2
        "#,
        )
        .bind(application_id)
        .bind(user_id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        let pricing_plan = row.pricing_plan_id.map(|id| AttachedPricingPlan {
            id,
            name: row.pricing_plan_name.clone().unwrap(),
            pricing_mode: row.pricing_mode.unwrap(),
            price_per_message_cents: row.price_per_message_cents,
            price_per_gb_cents: row.price_per_gb_cents,
            price_per_proof_cents: row.price_per_proof_cents,
            prepaid_amount_cents: row.prepaid_amount_cents,
            is_default: row.pricing_plan_is_default.unwrap_or(false),
            attached_at: row.pricing_plan_attached_at.unwrap_or_else(Utc::now),
        });

        let application = ApplicationWithUsage {
            application_id: row.application_id,
            user_id: row.developer_id,
            name: row.name,
            description: row.description,
            is_active: row.is_active,
            created_at: row.created_at,
            updated_at: row.updated_at,
            app_meta: row.app_meta,
            subscription_id: row.subscription_id,
            tier: row.tier,
            monthly_message_quota: row.monthly_message_quota,
            rate_limit_per_minute: row.rate_limit_per_minute,
            message_retention_seconds: row.message_retention_seconds,
            secret_key_id: row.secret_key_id,
            secret_key_prefix: row.secret_key_prefix,
            publishable_key_count: row.publishable_key_count,
            publishable_keys: row.publishable_keys,
            webhook_count: row.webhook_count,
            webhooks: row.webhooks,
            client_count: row.client_count,
            current_month_messages_sent: row.current_month_messages_sent,
            current_month_messages_received: row.current_month_messages_received,
            current_month_proofs_verified: row.current_month_proofs_verified,
            current_month_bytes_stored: row.current_month_bytes_stored,
            current_month_bytes_sent: row.current_month_bytes_sent,
            current_month_bytes_received: row.current_month_bytes_received,
            current_month_rate_limit_hits: row.current_month_rate_limit_hits,
            quota_usage_percentage: row.quota_usage_percentage,
            bandwidth_quota_usage_percentage: row.bandwidth_quota_usage_percentage,
            current_month_revenue_cents: row.current_month_revenue_cents,
            billable_clients_count: row.billable_clients_count,
            lifetime_messages_sent: row.lifetime_messages_sent,
        };

        Ok(ApplicationWithUsageAndPricingPlan {
            application,
            pricing_plan,
        })
    }

    /// List applications with summary data for pagination with optional filters
    pub async fn list_user_applications<'c, E>(
        exec: E,
        user_id: Uuid,
        page: i64,
        page_size: i64,
        search: Option<&str>,
        sort: Option<&str>,
        sort_order: Option<&str>,
        filter_active: Option<bool>,
        filter_inactive: Option<bool>,
        tier: Option<&str>,
    ) -> Result<PaginatedApplicationsSummary>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let offset = (page - 1).max(0) * page_size;

        // Build WHERE clause - using expression index for search
        let mut where_conditions: Vec<String> = vec!["developer_id = $1".to_string()];

        if let Some(_search) = search {
            where_conditions.push("lower(name) = lower($2)".to_string());
        }

        if let Some(active) = filter_active {
            if active {
                where_conditions.push("is_active = true".to_string());
            }
        }

        if let Some(inactive) = filter_inactive {
            if inactive {
                where_conditions.push("is_active = false".to_string());
            }
        }

        if let Some(_tier) = tier {
            where_conditions.push("lower(tier::text) = lower($2)".to_string());
        }

        if where_conditions.len() == 1 {
            where_conditions[0].clone()
        } else {
            where_conditions.join(" AND ")
        };

        // Build ORDER BY clause
        let (order_field, order_direction) = match (sort, sort_order) {
            (Some("name"), Some("asc")) => ("name", "ASC"),
            (Some("name"), Some("desc") | None) => ("name", "DESC"),
            (Some("createdAt"), Some("asc") | None) => ("created_at", "ASC"),
            (Some("createdAt"), Some("desc")) => ("created_at", "DESC"),
            (Some("updatedAt"), Some("asc") | None) => ("updated_at", "ASC"),
            (Some("updatedAt"), Some("desc")) => ("updated_at", "DESC"),
            (Some("quotaUsage"), Some("asc") | None) => {
                ("CAST(quota_usage_percentage AS NUMERIC)", "ASC")
            }
            (Some("quotaUsage"), Some("desc")) => {
                ("CAST(quota_usage_percentage AS NUMERIC)", "DESC")
            }
            _ => ("created_at", "DESC"),
        };

        // Use sequential parameter indices so they are contiguous regardless of which optional
        // filters are present. $1 is user_id; start next index at 2.
        let mut next_param = 2;
        let mut where_sql_parts: Vec<String> = vec!["developer_id = $1".to_string()];

        if let Some(_search) = search {
            where_sql_parts.push(format!("lower(name) LIKE lower(${})", next_param));
            next_param += 1;
        }

        if let Some(active) = filter_active {
            if active {
                where_sql_parts.push("is_active = true".to_string());
            }
        }

        if let Some(inactive) = filter_inactive {
            if inactive {
                where_sql_parts.push("is_active = false".to_string());
            }
        }

        if let Some(_tier) = tier {
            where_sql_parts.push(format!("lower(tier::text) = lower(${})", next_param));
            next_param += 1;
        }

        let where_sql = where_sql_parts.join(" AND ");

        // LIMIT and OFFSET use the next available parameters
        let limit_param = next_param;
        let offset_param = next_param + 1;

        let sql = format!(
            r#"
            SELECT
                application_id, name, description, is_active,
                created_at, updated_at, tier::text AS "tier",
                monthly_message_quota, publishable_key_count,
                webhook_count, client_count, quota_usage_percentage, bandwidth_quota_usage_percentage,
                current_month_revenue_cents, billable_clients_count,
                COUNT(*) OVER() AS total_count
            FROM mv_applications_with_usage
            WHERE {}
            ORDER BY {} {}
            LIMIT ${} OFFSET ${}
            "#,
            where_sql, order_field, order_direction, limit_param, offset_param
        );

        let mut query = sqlx::query_as::<_, ApplicationSummaryFromView>(&sql);

        // Bind in the same order: user_id, (search?), (tier?), page_size, offset
        query = query.bind(user_id);

        if let Some(_search) = search {
            let search_pattern = format!("%{}%", _search);
            query = query.bind(search_pattern);
        }

        if let Some(_tier) = tier {
            let tier_lower = _tier.to_lowercase();
            query = query.bind(tier_lower);
        }

        query = query.bind(page_size);
        query = query.bind(offset);

        let rows = query.fetch_all(exec).await?;

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
                bandwidth_quota_usage_percentage: r.bandwidth_quota_usage_percentage,
                current_month_revenue_cents: r.current_month_revenue_cents,
                billable_clients_count: r.billable_clients_count,
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

    pub async fn get_unified_quota_warnings(
        db: &sqlx::PgPool,
        user_id: Uuid,
        threshold: Option<Decimal>,
        page: i64,
        page_size: i64,
        quota_type: QuotaType,
    ) -> Result<PaginatedQuotaWarnings> {
        let threshold = threshold.unwrap_or_else(|| Decimal::from(80));
        let offset = (page - 1).max(0) * page_size;

        let (filter_clause, order_clause) = match quota_type {
            QuotaType::Messages => (
                "quota_usage_percentage >= $2",
                "quota_usage_percentage DESC",
            ),
            QuotaType::Bandwidth => (
                "bandwidth_quota_usage_percentage >= $2",
                "bandwidth_quota_usage_percentage DESC",
            ),
            QuotaType::Any => (
                "(quota_usage_percentage >= $2 OR bandwidth_quota_usage_percentage >= $2)",
                "GREATEST(quota_usage_percentage, bandwidth_quota_usage_percentage) DESC",
            ),
        };

        let sql = format!(
            r#"
            SELECT
                application_id, name as application_name, quota_usage_percentage, bandwidth_quota_usage_percentage,
                COUNT(*) OVER() AS total_count
            FROM mv_applications_with_usage
            WHERE developer_id = $1
                AND {}
                AND is_active = true
            ORDER BY {}
            LIMIT $3 OFFSET $4
            "#,
            filter_clause, order_clause
        );

        let rows = sqlx::query_as::<_, QuotaWarningWithCount>(&sql)
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
                bandwidth_quota_usage_percentage: r.bandwidth_quota_usage_percentage,
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
            critical_quota_apps AS "critical_quota_apps!",
            critical_bandwidth_quota_apps AS "critical_bandwidth_quota_apps!",
            total_monthly_revenue_cents AS "total_monthly_revenue_cents!"
        FROM get_user_usage_summary($1)
        "#,
            user_id
        )
        .fetch_one(db)
        .await?;

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    // Helper to build the same SQL as in list_user_applications so we can test placeholder numbering
    pub(crate) fn build_list_sql_for_test(
        search: Option<&str>,
        filter_active: Option<bool>,
        filter_inactive: Option<bool>,
        tier: Option<&str>,
        sort: Option<&str>,
        sort_order: Option<&str>,
    ) -> String {
        let mut where_sql_parts: Vec<String> = vec!["developer_id = $1".to_string()];
        let mut next_param = 2;

        if let Some(_search) = search {
            where_sql_parts.push(format!("lower(name) LIKE lower(${})", next_param));
            next_param += 1;
        }

        if let Some(active) = filter_active {
            if active {
                where_sql_parts.push("is_active = true".to_string());
            }
        }

        if let Some(inactive) = filter_inactive {
            if inactive {
                where_sql_parts.push("is_active = false".to_string());
            }
        }

        if let Some(_tier) = tier {
            where_sql_parts.push(format!("lower(tier::text) = lower(${})", next_param));
            next_param += 1;
        }

        let where_sql = where_sql_parts.join(" AND ");

        let (order_field, order_direction) = match (sort, sort_order) {
            (Some("name"), Some("asc")) => ("name", "ASC"),
            (Some("name"), Some("desc") | None) => ("name", "DESC"),
            (Some("createdAt"), Some("asc") | None) => ("created_at", "ASC"),
            (Some("createdAt"), Some("desc")) => ("created_at", "DESC"),
            (Some("updatedAt"), Some("asc") | None) => ("updated_at", "ASC"),
            (Some("updatedAt"), Some("desc")) => ("updated_at", "DESC"),
            (Some("quotaUsage"), Some("asc") | None) => {
                ("CAST(quota_usage_percentage AS NUMERIC)", "ASC")
            }
            (Some("quotaUsage"), Some("desc")) => {
                ("CAST(quota_usage_percentage AS NUMERIC)", "DESC")
            }
            _ => ("created_at", "DESC"),
        };

        let limit_param = next_param;
        let offset_param = next_param + 1;

        format!(
            r#"
            SELECT
                application_id, name, description, is_active,
                created_at, updated_at, tier::text AS "tier",
                monthly_message_quota, publishable_key_count,
                webhook_count, client_count, quota_usage_percentage, bandwidth_quota_usage_percentage,
                current_month_revenue_cents, billable_clients_count,
                COUNT(*) OVER() AS total_count
            FROM mv_applications_with_usage
            WHERE {}
            ORDER BY {} {}
            LIMIT ${} OFFSET ${}
            "#,
            where_sql, order_field, order_direction, limit_param, offset_param
        )
    }

    fn extract_placeholders(sql: &str) -> Vec<usize> {
        let re = Regex::new(r"\$(\d+)").unwrap();
        let mut nums: Vec<usize> = re
            .captures_iter(sql)
            .map(|c| c[1].parse::<usize>().unwrap())
            .collect();
        nums.sort_unstable();
        nums.dedup();
        nums
    }

    fn assert_placeholders_contiguous(sql: &str, expected_max: usize) {
        let nums = extract_placeholders(sql);
        let max = *nums.last().unwrap();
        assert_eq!(max, expected_max);
        let expected: Vec<usize> = (1..=max).collect();
        assert_eq!(
            nums, expected,
            "placeholders are not contiguous: {:?}",
            nums
        );
    }

    #[test]
    fn placeholders_no_filters_are_contiguous() {
        let sql = build_list_sql_for_test(None, None, None, None, None, None);
        // expected placeholders: $1 (user), $2 (limit), $3 (offset)
        assert_placeholders_contiguous(&sql, 3);
    }

    #[test]
    fn placeholders_search_only_are_contiguous() {
        let sql = build_list_sql_for_test(Some("foo"), None, None, None, None, None);
        // expected placeholders: $1 (user), $2 (search), $3 (limit), $4 (offset)
        assert_placeholders_contiguous(&sql, 4);
    }

    #[test]
    fn placeholders_tier_only_are_contiguous() {
        let sql = build_list_sql_for_test(None, None, None, Some("pro"), None, None);
        // expected placeholders: $1 (user), $2 (tier), $3 (limit), $4 (offset)
        assert_placeholders_contiguous(&sql, 4);
    }

    #[test]
    fn placeholders_search_and_tier_are_contiguous() {
        let sql = build_list_sql_for_test(Some("foo"), None, None, Some("pro"), None, None);
        // expected placeholders: $1 (user), $2 (search), $3 (tier), $4 (limit), $5 (offset)
        assert_placeholders_contiguous(&sql, 5);
    }
}
