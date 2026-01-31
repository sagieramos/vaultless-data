// vaultless-core/src/models/pricing/snapshot.rs

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use bigdecimal::FromPrimitive;

use super::enums::PricingMode;

// =============================================================================
// PRICING SNAPSHOT
// =============================================================================

/// Frozen snapshot of pricing for billing purposes
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PricingSnapshot {
    pub id: Uuid,  // Add id field to match the usage in the billing service
    pub plan_id: Uuid,
    pub plan_name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub platform_fee_percent: Option<rust_decimal::Decimal>,
    pub currency: Option<String>,
}

impl PricingSnapshot {
    pub fn get_price_per_message(&self) -> Decimal {
        Decimal::from(self.price_per_message_cents.unwrap_or(0))
    }

    pub fn get_price_per_byte(&self) -> Decimal {
        // Convert price per GB to price per byte
        let price_per_gb = self.price_per_gb_cents.unwrap_or(0) as f64;
        let bytes_per_gb = 1024.0 * 1024.0 * 1024.0;
        Decimal::from_f64(price_per_gb / bytes_per_gb).unwrap_or(Decimal::ZERO)
    }

    pub fn get_price_per_proof(&self) -> Decimal {
        Decimal::from(self.price_per_proof_cents.unwrap_or(0))
    }
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
