// vaultless-core/src/models/pricing/dto.rs

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

// =============================================================================
// CREATE PRICING PLAN
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreatePricingPlan {
    pub developer_id: Uuid,
    pub name: String,
    pub pricing_mode: super::PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
}

// =============================================================================
// ATTACH PRICING PLAN TO APPLICATION
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AttachPricingPlan {
    pub pricing_plan_id: Uuid,
    pub is_default: Option<bool>,
}

// =============================================================================
// CREATE CLIENT SUBSCRIPTION
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateClientSubscription {
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub pricing_plan_id: Uuid,
}

// =============================================================================
// UPDATE SUBSCRIPTION STATUS
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateSubscriptionStatus {
    pub status: super::SubscriptionStatus,
}

// =============================================================================
// CREATE BILLING PERIOD
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateBillingPeriod {
    pub application_id: Uuid,
    pub developer_id: Uuid,
    pub period_start: chrono::DateTime<chrono::Utc>,
    pub period_end: chrono::DateTime<chrono::Utc>,
}

// =============================================================================
// CLOSE BILLING PERIOD
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseBillingPeriod {
    pub status: super::BillingPeriodStatus,
}

// =============================================================================
// CREATE INVOICE
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateInvoice {
    pub billing_period_id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub developer_id: Uuid,
}

// =============================================================================
// UPDATE INVOICE STATUS
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateInvoiceStatus {
    pub status: super::InvoiceStatus,
}
