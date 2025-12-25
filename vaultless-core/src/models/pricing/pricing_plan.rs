// vaultless-core/src/models/pricing/pricing_plan.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

use super::{
    dto::CreatePricingPlan,
    enums::PricingMode,
    snapshot::PricingSnapshot,
};

// =============================================================================
// PRICING PLAN
// =============================================================================

/// Developer-created pricing plan for an application
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PricingPlan {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub created_at: DateTime<Utc>,
}

impl PricingPlan {
    /// Create a pricing snapshot from this plan
    pub fn to_snapshot(&self) -> PricingSnapshot {
        PricingSnapshot {
            plan_id: self.id,
            plan_name: self.name.clone(),
            pricing_mode: self.pricing_mode,
            price_per_message_cents: self.price_per_message_cents,
            price_per_gb_cents: self.price_per_gb_cents,
            price_per_proof_cents: self.price_per_proof_cents,
            prepaid_amount_cents: self.prepaid_amount_cents,
        }
    }

    /// Create a new pricing plan
    pub async fn create<'c, E>(executor: E, input: CreatePricingPlan) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let plan = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO pricing_plans (
                developer_id, name, pricing_mode,
                price_per_message_cents, price_per_gb_cents, price_per_proof_cents,
                prepaid_amount_cents
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(input.developer_id)
        .bind(&input.name)
        .bind(input.pricing_mode)
        .bind(input.price_per_message_cents)
        .bind(input.price_per_gb_cents)
        .bind(input.price_per_proof_cents)
        .bind(input.prepaid_amount_cents)
        .fetch_one(executor)
        .await?;

        Ok(plan)
    }

    /// Find a plan by ID
    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM pricing_plans WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Pricing plan not found".into()))
    }

    /// Find all plans for a developer
    pub async fn find_by_developer<'c, E>(executor: E, developer_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let plans = sqlx::query_as::<_, Self>(
            "SELECT * FROM pricing_plans WHERE developer_id = $1 ORDER BY created_at DESC",
        )
        .bind(developer_id)
        .fetch_all(executor)
        .await?;

        Ok(plans)
    }

    /// Delete a plan
    pub async fn delete<'c, E>(executor: E, id: Uuid, developer_id: Uuid) -> Result<bool>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let result = sqlx::query(
            "DELETE FROM pricing_plans WHERE id = $1 AND developer_id = $2",
        )
        .bind(id)
        .bind(developer_id)
        .execute(executor)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}
