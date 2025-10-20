use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};
use crate::types::SubscriptionTier;

/// Invoice represents a billing statement for a user
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Invoice {
    pub id: Uuid,
    pub user_id: Uuid,
    pub api_key_id: Option<Uuid>, // Null for account-level charges
    
    // Stripe integration
    pub stripe_invoice_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    
    // Amounts (in cents)
    pub subtotal_cents: i64,
    pub tax_cents: i64,
    pub discount_cents: i64,
    pub total_cents: i64,
    pub amount_paid_cents: i64,
    pub amount_due_cents: i64,
    
    // Invoice details
    pub currency: String, // USD, EUR, etc.
    pub invoice_number: String, // INV-2025-001234
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
    
    // Metadata
    pub metadata: Option<sqlx::types::JsonValue>,
}

/// Invoice line items (for itemized billing)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct InvoiceLineItem {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub api_key_id: Option<Uuid>,
    
    // Item details
    pub description: String,
    pub item_type: LineItemType,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub amount_cents: i64,
    
    // Metadata
    pub metadata: Option<sqlx::types::JsonValue>,
    pub created_at: DateTime<Utc>,
}

/// Payment transactions
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Payment {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub user_id: Uuid,
    
    // Stripe integration
    pub stripe_payment_intent_id: Option<String>,
    pub stripe_charge_id: Option<String>,
    
    // Payment details
    pub amount_cents: i64,
    pub currency: String,
    pub payment_method: PaymentMethod,
    pub status: PaymentStatus,
    
    // Card/Bank details (last 4 digits only)
    pub card_last4: Option<String>,
    pub card_brand: Option<String>, // visa, mastercard, etc.
    
    // Failure tracking
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub retry_count: i32,
    
    // Dates
    pub processed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    
    // Metadata
    pub metadata: Option<sqlx::types::JsonValue>,
}

/// Subscription management (linked to Stripe)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Subscription {
    pub id: Uuid,
    pub user_id: Uuid,
    
    // Stripe integration
    pub stripe_subscription_id: String,
    pub stripe_customer_id: String,
    pub stripe_price_id: String,
    
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
    
    // Metadata
    pub metadata: Option<sqlx::types::JsonValue>,
}

/// Credit balance for overage or prepaid credits
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CreditBalance {
    pub id: Uuid,
    pub user_id: Uuid,
    
    // Balance (in cents)
    pub balance_cents: i64,
    pub reserved_cents: i64, // Reserved for pending invoices
    pub available_cents: i64, // balance - reserved
    
    // Currency
    pub currency: String,
    
    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Credit transactions (for audit trail)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CreditTransaction {
    pub id: Uuid,
    pub user_id: Uuid,
    pub credit_balance_id: Uuid,
    
    // Transaction details
    pub amount_cents: i64,
    pub transaction_type: CreditTransactionType,
    pub description: String,
    
    // Related entities
    pub invoice_id: Option<Uuid>,
    pub payment_id: Option<Uuid>,
    
    // Timestamps
    pub created_at: DateTime<Utc>,
    
    // Metadata
    pub metadata: Option<sqlx::types::JsonValue>,
}

// ============================================================================
// ENUMS
// ============================================================================

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
#[sqlx(type_name = "line_item_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum LineItemType {
    Subscription,      // Monthly subscription fee
    MessageOverage,    // Extra messages beyond quota
    StorageOverage,    // Extra storage beyond quota
    ProofVerification, // Pay-per-verification
    Setup,             // One-time setup fee
    Discount,          // Promo discount
    Tax,               // Sales tax
    Credit,            // Applied credit
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "payment_method", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    Card,
    BankTransfer,
    Crypto,
    PayPal,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "credit_transaction_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum CreditTransactionType {
    Purchase,      // User bought credits
    Bonus,         // Promotional credit
    Refund,        // Refund to credit balance
    Applied,       // Credit applied to invoice
    Expired,       // Credit expired
    Adjustment,    // Admin adjustment
}

// ============================================================================
// IMPLEMENTATIONS
// ============================================================================

impl Invoice {
    /// Create a new invoice
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        api_key_id: Option<Uuid>,
        subtotal_cents: i64,
        tax_cents: i64,
        discount_cents: i64,
        billing_period_start: DateTime<Utc>,
        billing_period_end: DateTime<Utc>,
        due_date: DateTime<Utc>,
        description: Option<String>,
    ) -> Result<Self> {
        let total_cents = subtotal_cents + tax_cents - discount_cents;
        let invoice_number = Self::generate_invoice_number(pool).await?;

        let invoice = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO invoices (
                user_id, api_key_id, subtotal_cents, tax_cents, discount_cents,
                total_cents, amount_due_cents, currency, invoice_number,
                description, status, billing_period_start, billing_period_end, due_date
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(api_key_id)
        .bind(subtotal_cents)
        .bind(tax_cents)
        .bind(discount_cents)
        .bind(total_cents)
        .bind(total_cents) // amount_due = total initially
        .bind("USD")
        .bind(invoice_number)
        .bind(description)
        .bind(InvoiceStatus::Open)
        .bind(billing_period_start)
        .bind(billing_period_end)
        .bind(due_date)
        .fetch_one(pool)
        .await?;

        Ok(invoice)
    }

    /// Generate unique invoice number
    async fn generate_invoice_number(pool: &PgPool) -> Result<String> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices")
            .fetch_one(pool)
            .await?;

        let now = Utc::now();
        Ok(format!("INV-{}-{:06}", now.format("%Y%m"), count + 1))
    }

    /// Find invoice by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<Self> {
        let invoice = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM invoices 
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Invoice not found".to_string()))?;

        Ok(invoice)
    }

    /// List invoices for a user
    pub async fn list_for_user(
        pool: &PgPool,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>> {
        let invoices = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM invoices 
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(invoices)
    }

    /// Mark invoice as paid
    pub async fn mark_as_paid(
        pool: &PgPool,
        id: Uuid,
        amount_paid_cents: i64,
    ) -> Result<Self> {
        let invoice = sqlx::query_as::<_, Self>(
            r#"
            UPDATE invoices 
            SET 
                paid = true,
                status = 'paid',
                amount_paid_cents = $2,
                amount_due_cents = 0,
                paid_at = NOW(),
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(amount_paid_cents)
        .fetch_one(pool)
        .await?;

        Ok(invoice)
    }

    /// Get unpaid invoices for a user
    pub async fn get_unpaid(pool: &PgPool, user_id: Uuid) -> Result<Vec<Self>> {
        let invoices = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM invoices 
            WHERE user_id = $1 AND paid = false
            ORDER BY due_date ASC
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(invoices)
    }

    /// Get overdue invoices
    pub async fn get_overdue(pool: &PgPool) -> Result<Vec<Self>> {
        let invoices = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM invoices 
            WHERE paid = false AND due_date < NOW()
            ORDER BY due_date ASC
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(invoices)
    }
}

impl InvoiceLineItem {
    /// Add line item to invoice
    pub async fn create(
        pool: &PgPool,
        invoice_id: Uuid,
        api_key_id: Option<Uuid>,
        description: String,
        item_type: LineItemType,
        quantity: i64,
        unit_price_cents: i64,
    ) -> Result<Self> {
        let amount_cents = quantity * unit_price_cents;

        let line_item = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO invoice_line_items (
                invoice_id, api_key_id, description, item_type,
                quantity, unit_price_cents, amount_cents
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(invoice_id)
        .bind(api_key_id)
        .bind(description)
        .bind(item_type)
        .bind(quantity)
        .bind(unit_price_cents)
        .bind(amount_cents)
        .fetch_one(pool)
        .await?;

        Ok(line_item)
    }

    /// Get line items for an invoice
    pub async fn list_for_invoice(pool: &PgPool, invoice_id: Uuid) -> Result<Vec<Self>> {
        let items = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM invoice_line_items 
            WHERE invoice_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(invoice_id)
        .fetch_all(pool)
        .await?;

        Ok(items)
    }
}

impl Payment {
    /// Record a payment
    pub async fn create(
        pool: &PgPool,
        invoice_id: Uuid,
        user_id: Uuid,
        amount_cents: i64,
        payment_method: PaymentMethod,
        stripe_payment_intent_id: Option<String>,
    ) -> Result<Self> {
        let payment = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO payments (
                invoice_id, user_id, amount_cents, currency,
                payment_method, status, stripe_payment_intent_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(invoice_id)
        .bind(user_id)
        .bind(amount_cents)
        .bind("USD")
        .bind(payment_method)
        .bind(PaymentStatus::Pending)
        .bind(stripe_payment_intent_id)
        .fetch_one(pool)
        .await?;

        Ok(payment)
    }

    /// Update payment status
    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: PaymentStatus,
        failure_code: Option<String>,
        failure_message: Option<String>,
    ) -> Result<Self> {
        let payment = sqlx::query_as::<_, Self>(
            r#"
            UPDATE payments 
            SET 
                status = $2,
                failure_code = $3,
                failure_message = $4,
                processed_at = CASE WHEN $2 = 'succeeded' THEN NOW() ELSE processed_at END,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(status)
        .bind(failure_code)
        .bind(failure_message)
        .fetch_one(pool)
        .await?;

        Ok(payment)
    }

    /// Get payments for an invoice
    pub async fn list_for_invoice(pool: &PgPool, invoice_id: Uuid) -> Result<Vec<Self>> {
        let payments = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM payments 
            WHERE invoice_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(invoice_id)
        .fetch_all(pool)
        .await?;

        Ok(payments)
    }
}

impl Subscription {
    /// Create or update subscription
    pub async fn upsert(
        pool: &PgPool,
        user_id: Uuid,
        stripe_subscription_id: String,
        stripe_customer_id: String,
        stripe_price_id: String,
        tier: SubscriptionTier,
        amount_cents: i64,
        current_period_start: DateTime<Utc>,
        current_period_end: DateTime<Utc>,
        status: SubscriptionStatus,
    ) -> Result<Self> {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO subscriptions (
                user_id, stripe_subscription_id, stripe_customer_id, stripe_price_id,
                tier, amount_cents, currency, billing_cycle, status,
                current_period_start, current_period_end
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (stripe_subscription_id)
            DO UPDATE SET
                status = $9,
                current_period_start = $10,
                current_period_end = $11,
                updated_at = NOW()
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(stripe_subscription_id)
        .bind(stripe_customer_id)
        .bind(stripe_price_id)
        .bind(tier)
        .bind(amount_cents)
        .bind("USD")
        .bind(BillingCycle::Monthly)
        .bind(status)
        .bind(current_period_start)
        .bind(current_period_end)
        .fetch_one(pool)
        .await?;

        Ok(subscription)
    }

    /// Get active subscription for user
    pub async fn find_active(pool: &PgPool, user_id: Uuid) -> Result<Option<Self>> {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM subscriptions 
            WHERE user_id = $1 AND status IN ('trialing', 'active')
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        Ok(subscription)
    }

    /// Cancel subscription
    pub async fn cancel(
        pool: &PgPool,
        id: Uuid,
        cancel_at_period_end: bool,
        reason: Option<String>,
    ) -> Result<Self> {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            UPDATE subscriptions 
            SET 
                cancel_at_period_end = $2,
                canceled_at = NOW(),
                cancellation_reason = $3,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(cancel_at_period_end)
        .bind(reason)
        .fetch_one(pool)
        .await?;

        Ok(subscription)
    }
}

impl CreditBalance {
    /// Get or create credit balance for user
    pub async fn get_or_create(pool: &PgPool, user_id: Uuid) -> Result<Self> {
        let balance = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO credit_balances (user_id, balance_cents, reserved_cents, available_cents, currency)
            VALUES ($1, 0, 0, 0, 'USD')
            ON CONFLICT (user_id)
            DO UPDATE SET updated_at = NOW()
            RETURNING *
            "#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        Ok(balance)
    }

    /// Add credits
    pub async fn add_credits(
        pool: &PgPool,
        user_id: Uuid,
        amount_cents: i64,
        transaction_type: CreditTransactionType,
        description: String,
    ) -> Result<Self> {
        let balance = sqlx::query_as::<_, Self>(
            r#"
            UPDATE credit_balances 
            SET 
                balance_cents = balance_cents + $2,
                available_cents = available_cents + $2,
                updated_at = NOW()
            WHERE user_id = $1
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(amount_cents)
        .fetch_one(pool)
        .await?;

        // Record transaction
        CreditTransaction::create(
            pool,
            user_id,
            balance.id,
            amount_cents,
            transaction_type,
            description,
            None,
            None,
        )
        .await?;

        Ok(balance)
    }

    /// Deduct credits
    pub async fn deduct_credits(
        pool: &PgPool,
        user_id: Uuid,
        amount_cents: i64,
        invoice_id: Option<Uuid>,
        description: String,
    ) -> Result<Self> {
        let balance = sqlx::query_as::<_, Self>(
            r#"
            UPDATE credit_balances 
            SET 
                balance_cents = balance_cents - $2,
                available_cents = available_cents - $2,
                updated_at = NOW()
            WHERE user_id = $1 AND available_cents >= $2
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(amount_cents)
        .fetch_one(pool)
        .await?;

        // Record transaction
        CreditTransaction::create(
            pool,
            user_id,
            balance.id,
            -amount_cents,
            CreditTransactionType::Applied,
            description,
            invoice_id,
            None,
        )
        .await?;

        Ok(balance)
    }
}

impl CreditTransaction {
    /// Create credit transaction
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        credit_balance_id: Uuid,
        amount_cents: i64,
        transaction_type: CreditTransactionType,
        description: String,
        invoice_id: Option<Uuid>,
        payment_id: Option<Uuid>,
    ) -> Result<Self> {
        let transaction = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO credit_transactions (
                user_id, credit_balance_id, amount_cents, transaction_type,
                description, invoice_id, payment_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(credit_balance_id)
        .bind(amount_cents)
        .bind(transaction_type)
        .bind(description)
        .bind(invoice_id)
        .bind(payment_id)
        .fetch_one(pool)
        .await?;

        Ok(transaction)
    }

    /// List transactions for user
    pub async fn list_for_user(
        pool: &PgPool,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>> {
        let transactions = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM credit_transactions 
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(transactions)
    }
}