// vaultless-core/src/models/pricing/snapshot.rs

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::enums::PricingMode;

// =============================================================================
// PRICING SNAPSHOT
// =============================================================================

/// Frozen snapshot of pricing for billing purposes
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PricingSnapshot {
    pub plan_id: Uuid,
    pub plan_name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
}

// =============================================================================
// REVENUE SNAPSHOT
// =============================================================================

/// Revenue breakdown for a client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueSnapshot {
    pub message_revenue_cents: i64,
    pub storage_revenue_cents: i64,
    pub bandwidth_revenue_cents: i64,
    pub proof_revenue_cents: i64,
    pub total_revenue_cents: i64,
}

impl RevenueSnapshot {
    /// Create a new revenue snapshot with all zeros
    pub fn zero() -> Self {
        Self {
            message_revenue_cents: 0,
            storage_revenue_cents: 0,
            bandwidth_revenue_cents: 0,
            proof_revenue_cents: 0,
            total_revenue_cents: 0,
        }
    }
}
