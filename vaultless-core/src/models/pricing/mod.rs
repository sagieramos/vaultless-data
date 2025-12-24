// vaultless-core/src/models/pricing/mod.rs
// Pricing and billing models for client subscriptions, billing periods, and invoices

pub mod dto;

// Sub-modules for each model
pub mod application_pricing_plan;
pub mod billing_period;
pub mod client_billing_usage;
pub mod client_invoice;
pub mod client_subscription;
pub mod enums;
pub mod pricing_plan;
pub mod snapshot;

// Re-export all public types for convenient access
pub use application_pricing_plan::ApplicationPricingPlan;
pub use billing_period::BillingPeriod;
pub use client_billing_usage::ClientBillingUsage;
pub use client_invoice::ClientInvoice;
pub use client_subscription::ClientSubscription;
pub use dto::{
    AttachPricingPlan, CloseBillingPeriod, CreateBillingPeriod, CreateClientSubscription,
    CreateInvoice, CreatePricingPlan, UpdateInvoiceStatus, UpdateSubscriptionStatus,
};
pub use enums::{BillingPeriodStatus, InvoiceStatus, PricingMode, SubscriptionStatus};
pub use pricing_plan::PricingPlan;
pub use snapshot::{PricingSnapshot, RevenueSnapshot};
