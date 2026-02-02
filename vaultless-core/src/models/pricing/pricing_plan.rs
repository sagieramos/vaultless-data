use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json;
use sqlx::{Executor, FromRow, Postgres};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

use super::{dto::CreatePricingPlan, enums::PricingMode, snapshot::PricingSnapshot};

#[derive(sqlx::FromRow)]
struct PricingPlanPageRow {
    #[sqlx(flatten)]
    plan: PricingPlan,
    total_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Paginated<T> {
    pub items: Vec<T>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

#[derive(sqlx::FromRow)]
struct PricingPlanWithAttachmentCountPageRow {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub attached_application_count: i64,
    pub total_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingPlanWithAttachmentCount {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub attached_app_count: i64,
    pub created_at: DateTime<Utc>,
}

impl From<PricingPlanWithAttachmentCountPageRow> for PricingPlanWithAttachmentCount {
    fn from(row: PricingPlanWithAttachmentCountPageRow) -> Self {
        Self {
            id: row.id,
            developer_id: row.developer_id,
            name: row.name,
            pricing_mode: row.pricing_mode,
            price_per_message_cents: row.price_per_message_cents,
            price_per_gb_cents: row.price_per_gb_cents,
            price_per_proof_cents: row.price_per_proof_cents,
            prepaid_amount_cents: row.prepaid_amount_cents,
            created_at: row.created_at,
            attached_app_count: row.attached_application_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachedApplication {
    pub id: Option<Uuid>,
    pub name: Option<String>,
    pub is_active: Option<bool>,
    pub created_at: Option<DateTime<Utc>>,
    #[serde(rename = "quota_usage_percentage")]
    pub quota_usage_percentage_db: Option<BigDecimal>,
    #[serde(rename = "bandwidth_quota_usage_percentage")]
    pub bandwidth_quota_usage_percentage_db: Option<BigDecimal>,
    pub current_month_revenue_cents: Option<i64>,
}

impl AttachedApplication {
    pub fn quota_usage_percentage(&self) -> Option<Decimal> {
        self.quota_usage_percentage_db.as_ref().and_then(|bd| {
            bd.to_string().parse::<Decimal>().ok()
        })
    }

    pub fn bandwidth_quota_usage_percentage(&self) -> Option<Decimal> {
        self.bandwidth_quota_usage_percentage_db.as_ref().and_then(|bd| {
            bd.to_string().parse::<Decimal>().ok()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingPlanWithAttachedApps {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub attached_app_count: i64,
    pub attached_apps: Option<Vec<AttachedApplication>>,
    pub created_at: DateTime<Utc>,
}

impl From<PricingPlanWithAttachmentCount> for PricingPlanWithAttachedApps {
    fn from(plan: PricingPlanWithAttachmentCount) -> Self {
        Self {
            id: plan.id,
            developer_id: plan.developer_id,
            name: plan.name,
            pricing_mode: plan.pricing_mode,
            price_per_message_cents: plan.price_per_message_cents,
            price_per_gb_cents: plan.price_per_gb_cents,
            price_per_proof_cents: plan.price_per_proof_cents,
            prepaid_amount_cents: plan.prepaid_amount_cents,
            attached_app_count: plan.attached_app_count,
            attached_apps: None, // Will be populated separately if requested
            created_at: plan.created_at,
        }
    }
}

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
            id: Uuid::new_v4(), // Generate a new UUID for the snapshot
            plan_id: self.id,
            plan_name: self.name.clone(),
            pricing_mode: self.pricing_mode,
            price_per_message_cents: self.price_per_message_cents,
            price_per_gb_cents: self.price_per_gb_cents,
            price_per_proof_cents: self.price_per_proof_cents,
            prepaid_amount_cents: self.prepaid_amount_cents,
            platform_fee_percent: None, // Default to None, can be set elsewhere
            currency: Some("USD".to_string()), // Default to USD, can be customized
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

    async fn find<'c, E>(
        executor: E,
        developer_id: Uuid,
        id: Option<Uuid>,
        page: Option<i64>,
        page_size: Option<i64>,
    ) -> Result<Paginated<Self>>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let is_paginated = id.is_none();

        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(20);
        let offset = (page - 1) * page_size;

        let rows: Vec<PricingPlanPageRow> = sqlx::query_as(
            r#"
        SELECT
            p.*,
            COUNT(*) OVER() AS total_count
            FROM pricing_plans p
            WHERE p.developer_id = $1
            AND ($2::uuid IS NULL OR p.id = $2)
            ORDER BY p.created_at DESC
            LIMIT $3 OFFSET $4
        "#,
        )
        .bind(developer_id)
        .bind(id)
        .bind(if is_paginated { page_size } else { 1 })
        .bind(if is_paginated { offset } else { 0 })
        .fetch_all(executor)
        .await?;

        let total_count = rows.first().map(|r| r.total_count).unwrap_or(0);

        let total_pages = if is_paginated {
            (total_count as f64 / page_size as f64).ceil() as i64
        } else if total_count > 0 {
            1
        } else {
            0
        };

        let items = rows.into_iter().map(|r| r.plan).collect();

        Ok(Paginated {
            items,
            total_count,
            page: if is_paginated { page } else { 1 },
            page_size: if is_paginated { page_size } else { 1 },
            total_pages,
        })
    }

    pub async fn find_by_id<'c, E>(executor: E, developer_id: Uuid, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let result = Self::find(executor, developer_id, Some(id), None, None).await?;

        result
            .items
            .into_iter()
            .next()
            .ok_or_else(|| VaultlessError::NotFound("Pricing plan not found".into()))
    }

    /// Find a pricing plan by ID with optional attached applications
    pub async fn find_by_id_with_attached_apps<'c, E>(
        executor: E,
        developer_id: Uuid,
        id: Uuid,
        include_attached_apps: Option<u32>,  // Number of attached apps to include (None means don't include)
    ) -> Result<PricingPlanWithAttachedApps>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let limit = include_attached_apps.unwrap_or(0) as i32;

        let row = sqlx::query!(
            r#"
            SELECT
                plan_id as "plan_id!",
                plan_developer_id as "plan_developer_id!",
                plan_name as "plan_name!",
                plan_pricing_mode as "plan_pricing_mode!: PricingMode",
                plan_price_per_message_cents,
                plan_price_per_gb_cents,
                plan_price_per_proof_cents,
                plan_prepaid_amount_cents,
                plan_created_at as "plan_created_at!",
                plan_attached_app_count as "plan_attached_app_count!",
                attached_apps_json
            FROM find_pricing_plan_with_attached_apps($1, $2, $3)
            "#,
            id,
            developer_id,
            limit
        )
        .fetch_optional(executor)
        .await?;

        if let Some(db_row) = row {
            let attached_apps = if limit > 0 {
                match &db_row.attached_apps_json {
                    Some(json_val) => {
                        // Parse the JSON array of attached applications
                        let apps: Option<Vec<AttachedApplication>> = serde_json::from_value(json_val.clone()).ok();
                        apps
                    },
                    None => None,
                }
            } else {
                None
            };

            let pricing_plan_with_attached = PricingPlanWithAttachedApps {
                id: db_row.plan_id,
                developer_id: db_row.plan_developer_id,
                name: db_row.plan_name,
                pricing_mode: db_row.plan_pricing_mode,
                price_per_message_cents: db_row.plan_price_per_message_cents,
                price_per_gb_cents: db_row.plan_price_per_gb_cents,
                price_per_proof_cents: db_row.plan_price_per_proof_cents,
                prepaid_amount_cents: db_row.plan_prepaid_amount_cents,
                attached_app_count: db_row.plan_attached_app_count,
                attached_apps,
                created_at: db_row.plan_created_at,
            };

            Ok(pricing_plan_with_attached)
        } else {
            Err(VaultlessError::NotFound("Pricing plan not found".into()))
        }
    }

    pub async fn find_by_developer_paginated<'c, E>(
        executor: E,
        developer_id: Uuid,
        page: i64,
        page_size: i64,
    ) -> Result<Paginated<Self>>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        Self::find(executor, developer_id, None, Some(page), Some(page_size)).await
    }

    /// Delete a plan
    pub async fn delete<'c, E>(executor: E, id: Uuid, developer_id: Uuid) -> Result<bool>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // First check if the plan is attached to any applications
        let attached_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM application_pricing_plans WHERE pricing_plan_id = $1",
        )
        .bind(id)
        .fetch_one(executor.clone())
        .await?;

        if attached_count.0 > 0 {
            return Err(VaultlessError::BadRequest(
                "Cannot delete pricing plan that is attached to one or more applications"
                    .to_string(),
            ));
        }

        let result = sqlx::query("DELETE FROM pricing_plans WHERE id = $1 AND developer_id = $2")
            .bind(id)
            .bind(developer_id)
            .execute(executor)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Find all plans for a developer with their attachment counts
    pub async fn find_with_attachment_count<'c, E>(
        executor: E,
        developer_id: Uuid,
        plan_id: Option<Uuid>,
        page: Option<i64>,
        page_size: Option<i64>,
    ) -> Result<Paginated<PricingPlanWithAttachmentCount>>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let page = page.unwrap_or(1);
        let page_size = page_size.unwrap_or(1);
        let offset = (page - 1) * page_size;

        let rows: Vec<PricingPlanWithAttachmentCountPageRow> = sqlx::query_as(
            r#"
        SELECT
            p.id,
            p.developer_id,
            p.name,
            p.pricing_mode,
            p.price_per_message_cents,
            p.price_per_gb_cents,
            p.price_per_proof_cents,
            p.prepaid_amount_cents,
            p.created_at,
            COALESCE(attached_counts.attached_count, 0) AS attached_application_count,
            COUNT(*) OVER() AS total_count
        FROM pricing_plans p
        LEFT JOIN (
            SELECT pricing_plan_id, COUNT(*) AS attached_count
            FROM application_pricing_plans
            GROUP BY pricing_plan_id
        ) attached_counts ON p.id = attached_counts.pricing_plan_id
        WHERE p.developer_id = $1
          AND ($2::uuid IS NULL OR p.id = $2)
        ORDER BY p.created_at DESC
        LIMIT $3 OFFSET $4
        "#,
        )
        .bind(developer_id)
        .bind(plan_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(executor)
        .await?;

        let total_count = rows.first().map(|r| r.total_count).unwrap_or(0);
        let total_pages = if page_size > 0 {
            (total_count as f64 / page_size as f64).ceil() as i64
        } else {
            0
        };

        let items = rows.into_iter().map(Into::into).collect();

        Ok(Paginated {
            items,
            total_count,
            total_pages,
            page,
            page_size,
        })
    }
}
