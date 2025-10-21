// vaultless-core/src/models/billing.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::Result;
use crate::types::SubscriptionTier;

// ============================================================================
// PAYMENT GATEWAY ABSTRACTION
// ============================================================================

/// Payment gateway provider enum
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "payment_gateway", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PaymentGateway {
    Stripe,
    PayPal,
    Paystack,    // Popular in Africa
    Flutterwave, // Popular in Africa
    Razorpay,    // Popular in India
    Square,
    Braintree,
    Manual, // For wire transfers, checks, etc.
}

/// External reference for any payment gateway
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ExternalPaymentReference {
    pub id: Uuid,
    pub entity_type: String, // "invoice", "subscription", "payment", "customer"
    pub entity_id: Uuid,
    pub gateway: PaymentGateway,
    pub external_id: String, // Gateway's ID for this entity
    pub external_metadata: Option<sqlx::types::JsonValue>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// REFACTORED MODELS (Gateway-Agnostic)
// ============================================================================

/// Invoice - now gateway-agnostic
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Invoice {
    pub id: Uuid,
    pub user_id: Uuid,
    pub api_key_id: Option<Uuid>,

    // Gateway-agnostic fields
    pub payment_gateway: Option<PaymentGateway>,

    // Amounts (in cents)
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub discount_cents: i64,
    pub total_cents: i64,
    pub amount_paid_cents: i64,
    pub amount_due_cents: i64,

    // Invoice details
    pub currency: String,
    pub invoice_number: String,
    pub description: Option<String>,

    // Status
    pub status: InvoiceStatus,
    pub paid: bool,

    // Dates
    pub billing_period_start: DateTime<Utc>,
    pub billing_period_end: DateTime<Utc>,
    pub due_date: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,

    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Flexible metadata
    pub metadata: Option<sqlx::types::JsonValue>,
}

/// Subscription - gateway-agnostic
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Subscription {
    pub id: Uuid,
    pub user_id: Uuid,

    // Gateway identification
    pub payment_gateway: PaymentGateway,

    // Subscription details
    pub tier: SubscriptionTier,
    pub status: SubscriptionStatus,

    // Billing
    pub billing_cycle: BillingCycle,
    pub amount_cents: i64,
    pub currency: String,

    // Trial
    pub trial_end: Option<DateTime<Utc>>,
    pub trial_days: Option<i32>,

    // Current period
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,

    // Cancellation
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<DateTime<Utc>>,
    pub cancellation_reason: Option<String>,

    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Flexible metadata
    pub metadata: Option<sqlx::types::JsonValue>,
}

/// Payment - gateway-agnostic
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Payment {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub user_id: Uuid,

    // Gateway identification
    pub payment_gateway: PaymentGateway,

    // Payment details
    pub amount_cents: i64,
    pub currency: String,
    pub payment_method: PaymentMethod,
    pub status: PaymentStatus,

    // Card/Bank details (last 4 digits only)
    pub card_last4: Option<String>,
    pub card_brand: Option<String>,

    // Failure tracking
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub retry_count: i32,

    // Dates
    pub processed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Flexible metadata
    pub metadata: Option<sqlx::types::JsonValue>,
}

// ============================================================================
// EXTERNAL REFERENCE IMPLEMENTATIONS
// ============================================================================

impl ExternalPaymentReference {
    /// Create or update external reference
    pub async fn upsert(
        pool: &PgPool,
        entity_type: &str,
        entity_id: Uuid,
        gateway: PaymentGateway,
        external_id: String,
        metadata: Option<serde_json::Value>,
    ) -> Result<Self> {
        let reference = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO external_payment_references (
                entity_type, entity_id, gateway, external_id, external_metadata
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (entity_type, entity_id, gateway)
            DO UPDATE SET
                external_id = $4,
                external_metadata = $5,
                updated_at = NOW()
            RETURNING *
            "#,
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(gateway)
        .bind(external_id)
        .bind(metadata)
        .fetch_one(pool)
        .await?;

        Ok(reference)
    }

    /// Get external reference for an entity
    pub async fn find(
        pool: &PgPool,
        entity_type: &str,
        entity_id: Uuid,
        gateway: PaymentGateway,
    ) -> Result<Option<Self>> {
        let reference = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM external_payment_references
            WHERE entity_type = $1 AND entity_id = $2 AND gateway = $3
            "#,
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(gateway)
        .fetch_optional(pool)
        .await?;

        Ok(reference)
    }

    /// Find entity by external ID
    pub async fn find_by_external_id(
        pool: &PgPool,
        gateway: PaymentGateway,
        external_id: &str,
    ) -> Result<Option<Self>> {
        let reference = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM external_payment_references
            WHERE gateway = $1 AND external_id = $2
            "#,
        )
        .bind(gateway)
        .bind(external_id)
        .fetch_optional(pool)
        .await?;

        Ok(reference)
    }

    /// Delete external reference
    pub async fn delete(
        pool: &PgPool,
        entity_type: &str,
        entity_id: Uuid,
        gateway: PaymentGateway,
    ) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM external_payment_references
            WHERE entity_type = $1 AND entity_id = $2 AND gateway = $3
            "#,
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(gateway)
        .execute(pool)
        .await?;

        Ok(())
    }
}

// ============================================================================
// HELPER EXTENSIONS FOR MAIN MODELS
// ============================================================================

impl Invoice {
    /// Get external reference for this invoice
    pub async fn get_external_reference(
        &self,
        pool: &PgPool,
        gateway: PaymentGateway,
    ) -> Result<Option<ExternalPaymentReference>> {
        ExternalPaymentReference::find(pool, "invoice", self.id, gateway).await
    }

    /// Set external reference
    pub async fn set_external_reference(
        &self,
        pool: &PgPool,
        gateway: PaymentGateway,
        external_id: String,
        metadata: Option<serde_json::Value>,
    ) -> Result<ExternalPaymentReference> {
        ExternalPaymentReference::upsert(pool, "invoice", self.id, gateway, external_id, metadata)
            .await
    }
}

impl Subscription {
    /// Get external reference
    pub async fn get_external_reference(
        &self,
        pool: &PgPool,
    ) -> Result<Option<ExternalPaymentReference>> {
        ExternalPaymentReference::find(pool, "subscription", self.id, self.payment_gateway).await
    }

    /// Set external reference
    pub async fn set_external_reference(
        &self,
        pool: &PgPool,
        external_id: String,
        metadata: Option<serde_json::Value>,
    ) -> Result<ExternalPaymentReference> {
        ExternalPaymentReference::upsert(
            pool,
            "subscription",
            self.id,
            self.payment_gateway,
            external_id,
            metadata,
        )
        .await
    }
}

impl Payment {
    /// Get external reference
    pub async fn get_external_reference(
        &self,
        pool: &PgPool,
    ) -> Result<Option<ExternalPaymentReference>> {
        ExternalPaymentReference::find(pool, "payment", self.id, self.payment_gateway).await
    }

    /// Set external reference
    pub async fn set_external_reference(
        &self,
        pool: &PgPool,
        external_id: String,
        metadata: Option<serde_json::Value>,
    ) -> Result<ExternalPaymentReference> {
        ExternalPaymentReference::upsert(
            pool,
            "payment",
            self.id,
            self.payment_gateway,
            external_id,
            metadata,
        )
        .await
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "invoice_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Draft,
    Open,
    Paid,
    Void,
    Uncollectible,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "payment_method", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    Card,
    BankTransfer,
    Crypto,
    PayPal,
    MobileMoney, // For African markets
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "payment_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    Pending,
    Processing,
    Succeeded,
    Failed,
    Canceled,
    Refunded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "subscription_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Trialing,
    Active,
    PastDue,
    Canceled,
    Unpaid,
    Incomplete,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "billing_cycle", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum BillingCycle {
    Monthly,
    Yearly,
}
