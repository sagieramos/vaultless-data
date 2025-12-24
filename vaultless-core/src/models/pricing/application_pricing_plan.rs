// vaultless-core/src/models/pricing/application_pricing_plan.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use super::{
    dto::AttachPricingPlan,
    pricing_plan::PricingPlan,
};

// =============================================================================
// APPLICATION PRICING PLAN
// =============================================================================

/// Association between an application and its pricing plans
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApplicationPricingPlan {
    pub application_id: Uuid,
    pub pricing_plan_id: Uuid,
    pub is_default: bool,
    pub attached_at: DateTime<Utc>,
}

impl ApplicationPricingPlan {
    /// Attach a pricing plan to an application
    pub async fn attach<'c, E>(executor: E, input: AttachPricingPlan, application_id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Use a single query with CTE to handle default clearing and insertion atomically
        let plan = sqlx::query_as::<_, Self>(
            r#"
            WITH updated AS (
                UPDATE application_pricing_plans
                SET is_default = false
                WHERE application_id = $1
                  AND pricing_plan_id <> $2
                  AND $3 = true
                RETURNING application_id
            )
            INSERT INTO application_pricing_plans (application_id, pricing_plan_id, is_default)
            VALUES ($1, $2, $3)
            ON CONFLICT (application_id, pricing_plan_id)
            DO UPDATE SET is_default = $3
            RETURNING *
            "#,
        )
        .bind(application_id)
        .bind(input.pricing_plan_id)
        .bind(input.is_default.unwrap_or(false))
        .fetch_one(executor)
        .await?;

        Ok(plan)
    }

    /// Get all pricing plans attached to an application
    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let plans = sqlx::query_as::<_, Self>(
            "SELECT * FROM application_pricing_plans WHERE application_id = $1 ORDER BY attached_at DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(plans)
    }

    /// Get all pricing plans for an application with full plan details
    pub async fn find_by_application_with_plans<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<(Self, PricingPlan)>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Use a raw query with JOIN to get both at once
        #[derive(FromRow)]
        struct AppPlanWithPlan {
            application_id: Uuid,
            pricing_plan_id: Uuid,
            is_default: bool,
            attached_at: DateTime<Utc>,
            // Pricing plan fields
            plan_id: Uuid,
            developer_id: Uuid,
            name: String,
            plan_pricing_mode: super::enums::PricingMode,
            price_per_message_cents: Option<i64>,
            price_per_gb_cents: Option<i64>,
            price_per_proof_cents: Option<i64>,
            prepaid_amount_cents: Option<i64>,
            plan_created_at: DateTime<Utc>,
        }

        let rows: Vec<AppPlanWithPlan> = sqlx::query_as::<_, AppPlanWithPlan>(
            r#"
            SELECT
                app.application_id,
                app.pricing_plan_id,
                app.is_default,
                app.attached_at,
                plan.id as plan_id,
                plan.developer_id,
                plan.name,
                plan.pricing_mode as plan_pricing_mode,
                plan.price_per_message_cents,
                plan.price_per_gb_cents,
                plan.price_per_proof_cents,
                plan.prepaid_amount_cents,
                plan.created_at as plan_created_at
            FROM application_pricing_plans app
            JOIN pricing_plans plan ON app.pricing_plan_id = plan.id
            WHERE app.application_id = $1
            ORDER BY app.attached_at DESC
            "#,
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let app_plan = Self {
                    application_id: row.application_id,
                    pricing_plan_id: row.pricing_plan_id,
                    is_default: row.is_default,
                    attached_at: row.attached_at,
                };
                let pricing_plan = PricingPlan {
                    id: row.plan_id,
                    developer_id: row.developer_id,
                    name: row.name,
                    pricing_mode: row.plan_pricing_mode,
                    price_per_message_cents: row.price_per_message_cents,
                    price_per_gb_cents: row.price_per_gb_cents,
                    price_per_proof_cents: row.price_per_proof_cents,
                    prepaid_amount_cents: row.prepaid_amount_cents,
                    created_at: row.plan_created_at,
                };
                (app_plan, pricing_plan)
            })
            .collect())
    }

    /// Get the default pricing plan for an application with full plan details
    pub async fn get_default_with_plan<'c, E>(executor: E, application_id: Uuid) -> Result<Option<(Self, PricingPlan)>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let plans = Self::find_by_application_with_plans(executor, application_id).await?;
        Ok(plans.into_iter().find(|(app_plan, _)| app_plan.is_default))
    }

    /// Detach a pricing plan from an application
    pub async fn detach<'c, E>(executor: E, application_id: Uuid, pricing_plan_id: Uuid) -> Result<bool, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let result = sqlx::query(
            "DELETE FROM application_pricing_plans WHERE application_id = $1 AND pricing_plan_id = $2",
        )
        .bind(application_id)
        .bind(pricing_plan_id)
        .execute(executor)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}
