// vaultless-core/src/models/pricing/enums.rs

use serde::{Deserialize, Serialize};

// =============================================================================
// ENUMS
// =============================================================================

/// Pricing mode for a plan
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "pricing_mode_enum", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PricingMode {
    Postpaid,
    Prepaid,
    Free,
}

/// Subscription status for client subscriptions
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "subscription_status_enum", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Active,
    Paused,
    Cancelled,
}

/// Billing period status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum BillingPeriodStatus {
    Open,
    Closed,
    Invoiced,
}

/// Invoice status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Pending,
    Finalized,
    Paid,
    Failed,
}
