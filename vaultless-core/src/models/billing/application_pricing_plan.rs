use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ApplicationPricingPlan {
    pub application_id: Uuid,
    pub pricing_plan_id: Uuid,  // References the actual pricing plan
    pub is_default: bool,       // Is this the default plan for the application?
    pub attached_at: DateTime<Utc>,
}

impl ApplicationPricingPlan {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        application_id: Uuid,
        pricing_plan_id: Uuid,
        is_default: bool,
    ) -> Result<Self> {
        let plan = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO application_pricing_plans (
                application_id, pricing_plan_id, is_default
            )
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(application_id)
        .bind(pricing_plan_id)
        .bind(is_default)
        .fetch_one(&mut **tx)
        .await?;

        Ok(plan)
    }

    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let plans = sqlx::query_as::<_, Self>(
            "SELECT * FROM application_pricing_plans WHERE application_id = $1 ORDER BY attached_at",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(plans)
    }

    pub async fn find_default_for_application<'c, E>(executor: E, application_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let plan = sqlx::query_as::<_, Self>(
            "SELECT * FROM application_pricing_plans WHERE application_id = $1 AND is_default = true LIMIT 1",
        )
        .bind(application_id)
        .fetch_optional(executor)
        .await?;

        Ok(plan)
    }

    pub async fn find_by_ids<'c, E>(
        executor: E,
        application_id: Uuid,
        pricing_plan_id: Uuid,
    ) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let plan = sqlx::query_as::<_, Self>(
            "SELECT * FROM application_pricing_plans WHERE application_id = $1 AND pricing_plan_id = $2 LIMIT 1",
        )
        .bind(application_id)
        .bind(pricing_plan_id)
        .fetch_optional(executor)
        .await?;

        Ok(plan)
    }
}