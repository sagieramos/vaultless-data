use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx::{Executor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::Result,
    models::{
        billing::{
            ClientUsageCredit, CreditTransaction, DeveloperRevenueShare, PspAccount, PspPayout,
            ApplicationPricingPlan, ClientSubscription, ClientBillingUsage, ClientInvoice, BillingPeriod
        },
        pricing::snapshot::PricingSnapshot,
    },
};

pub struct BillingService;

impl BillingService {
    /// Process usage event and attribute revenue to developer
    /// This is the core function that implements the mental model:
    /// Credits unlock usage -> Usage creates entitlement -> PSP moves money
    pub async fn process_usage_event(
        tx: &mut Transaction<'_, Postgres>,
        client_id: Uuid,
        application_id: Uuid,
        developer_id: Uuid,
        pricing_snapshot: &PricingSnapshot,
        messages_sent: i64,
        messages_received: i64,
        bytes_sent: i64,
        bytes_received: i64,
        proofs_verified: i64,
        billing_period_id: Uuid,
    ) -> Result<i64> {  // Return remaining credits
        // First, check if the client has sufficient credits for this usage
        let client_credit = ClientUsageCredit::find_by_client(&mut **tx, client_id)
            .await?
            .ok_or_else(|| crate::error::VaultlessError::NotFound("Client usage credit not found".into()))?;

        // Calculate the cost of this usage based on the pricing snapshot
        let usage_cost = Self::calculate_usage_cost(
            pricing_snapshot,
            messages_sent,
            messages_received,
            bytes_sent,
            bytes_received,
            proofs_verified,
        );

        // Check if client has enough credits
        if client_credit.credit_balance < usage_cost {
            return Err(crate::error::VaultlessError::InsufficientCredits(
                "Client does not have enough credits for this usage".to_string(),
            ));
        }

        // Create revenue snapshot for this usage
        let revenue_snapshot = serde_json::json!({
            "pricing_snapshot_id": pricing_snapshot.id,
            "pricing_model": pricing_snapshot.pricing_mode,
            "price_per_message": pricing_snapshot.price_per_message_cents,
            "price_per_gb": pricing_snapshot.price_per_gb_cents,
            "price_per_proof": pricing_snapshot.price_per_proof_cents,
            "platform_fee_percent": pricing_snapshot.platform_fee_percent,
            "calculated_at": Utc::now(),
            "usage_cost": usage_cost
        });

        // Record the usage event
        let usage = ClientBillingUsage::create(
            tx,
            billing_period_id,
            client_id,
            application_id,
            messages_sent,
            messages_received,
            proofs_verified,
            bytes_sent + bytes_received,  // total_bytes_stored
            bytes_sent,  // total_bytes_sent
            bytes_received,  // total_bytes_received
            0,  // rate_limit_hits - assuming none for this usage
            developer_id,
            revenue_snapshot,
        ).await?;

        // Deduct credits from client
        let updated_credit = ClientUsageCredit::update_balance::<Transaction<'_, Postgres>>(tx, client_id, -usage_cost).await?;

        // Create a credit transaction record
        CreditTransaction::create(
            tx,
            client_id,
            application_id,
            "usage_deduction".to_string(),
            -usage_cost,
            Some(serde_json::json!({
                "usage_id": usage.id,
                "pricing_snapshot_id": pricing_snapshot.id,
                "messages_sent": messages_sent,
                "messages_received": messages_received,
                "bytes_sent": bytes_sent,
                "bytes_received": bytes_received,
                "proofs_verified": proofs_verified
            })),
            None,  // No related transaction for usage deduction
            Some(billing_period_id),
        ).await?;

        // Attribute revenue to developer based on usage
        Self::attribute_revenue_to_developer(
            tx,
            developer_id,
            application_id,
            billing_period_id,
            messages_sent,
            messages_received,
            bytes_sent,
            bytes_received,
            proofs_verified,
            usage_cost,
            pricing_snapshot,
        ).await?;

        Ok(updated_credit.credit_balance)
    }

    /// Calculate the cost of usage based on pricing snapshot
    fn calculate_usage_cost(
        pricing_snapshot: &PricingSnapshot,
        messages_sent: i64,
        messages_received: i64,
        bytes_sent: i64,
        bytes_received: i64,
        proofs_verified: i64,
    ) -> i64 {
        // This is a simplified calculation - in reality, pricing could be more complex
        let cost = (messages_sent as f64 * pricing_snapshot.get_price_per_message().to_f64().unwrap_or(0.0)) as i64 +
                   (messages_received as f64 * pricing_snapshot.get_price_per_message().to_f64().unwrap_or(0.0)) as i64 +
                   (bytes_sent as f64 * pricing_snapshot.get_price_per_byte().to_f64().unwrap_or(0.0)) as i64 +
                   (bytes_received as f64 * pricing_snapshot.get_price_per_byte().to_f64().unwrap_or(0.0)) as i64 +
                   (proofs_verified as f64 * pricing_snapshot.get_price_per_proof().to_f64().unwrap_or(0.0)) as i64;

        cost
    }

    /// Attribute revenue to developer based on usage
    async fn attribute_revenue_to_developer(
        tx: &mut Transaction<'_, Postgres>,
        developer_id: Uuid,
        application_id: Uuid,
        billing_period_id: Uuid,
        messages_sent: i64,
        messages_received: i64,
        bytes_sent: i64,
        bytes_received: i64,
        proofs_verified: i64,
        gross_revenue_cents: i64,
        pricing_snapshot: &PricingSnapshot,
    ) -> Result<()> {
        // Calculate platform fee
        let platform_fee_percent = pricing_snapshot.platform_fee_percent.unwrap_or(Decimal::from(10)); // Default to 10%
        let platform_fee_cents = (gross_revenue_cents as f64 * platform_fee_percent.to_f64().unwrap_or(0.1)) as i64;
        let net_revenue_cents = gross_revenue_cents - platform_fee_cents;

        // Create or update developer revenue share
        let _revenue_share = DeveloperRevenueShare::create(
            tx,
            developer_id,
            application_id,
            billing_period_id,
            messages_sent + messages_received,  // Total messages
            bytes_sent + bytes_received,       // Total bytes
            proofs_verified,
            gross_revenue_cents,
            platform_fee_percent,
            platform_fee_cents,
            net_revenue_cents,
            pricing_snapshot.currency.clone().unwrap_or("USD".to_string()),
        ).await?;

        Ok(())
    }

    /// Add credits to a client's account (credits are non-cash usage units)
    pub async fn add_credits_to_client(
        tx: &mut Transaction<'_, Postgres>,
        client_id: Uuid,
        application_id: Uuid,  // The application context for this credit addition
        credits_to_add: i64,
        cash_value_cents: i64,
    ) -> Result<()> {
        // Ensure this is a positive addition
        if credits_to_add <= 0 {
            return Err(crate::error::VaultlessError::InvalidInput(
                "Credits to add must be positive".to_string(),
            ));
        }

        // Update client's credit balance
        let updated_credit = ClientUsageCredit::update_balance::<Transaction<'_, Postgres>>(
            tx,
            client_id,
            credits_to_add, // Positive to increase balance
        )
        .await?;

        // Create a credit transaction record for the addition
        CreditTransaction::create(
            tx,
            client_id,
            application_id,
            "credit_purchase".to_string(),
            credits_to_add,
            Some(serde_json::json!({
                "cash_value_cents": cash_value_cents,
                "fx_conversion_locked": true  // At the time of purchase
            })),
            None,  // No related transaction
            None,  // Not associated with a billing period yet
        ).await?;

        Ok(())
    }

    /// Subscribe a client to an application pricing plan
    pub async fn subscribe_client_to_plan(
        tx: &mut Transaction<'_, Postgres>,
        client_id: Uuid,
        application_id: Uuid,
        pricing_plan_id: Uuid,
    ) -> Result<ClientSubscription> {
        // Check if the pricing plan is available for this application
        let app_plan = ApplicationPricingPlan::find_by_ids(&mut **tx, application_id, pricing_plan_id)
            .await?
            .ok_or_else(|| crate::error::VaultlessError::NotFound(
                "Pricing plan not available for this application".to_string()
            ))?;

        // Check if the client is already subscribed to this application
        if let Some(existing_subscription) = ClientSubscription::find_by_client_and_application(
            &mut **tx,
            client_id,
            application_id,
        ).await? {
            // Update the existing subscription to the new plan
            return Ok(ClientSubscription::update_status(
                &mut **tx,
                existing_subscription.id,
                "cancelled".to_string(),
            ).await?);
        }

        // Create pricing snapshot for this subscription
        let pricing_snapshot = serde_json::json!({
            "pricing_plan_id": pricing_plan_id,
            "attached_at": Utc::now(),
            "pricing_model": "subscription",  // or whatever the actual model is
            "terms": {}  // Include actual pricing terms here
        });

        // Create a new subscription
        let subscription = ClientSubscription::create(
            tx,
            client_id,
            application_id,
            pricing_plan_id,  // Using the pricing plan ID directly
            "active".to_string(),
            Utc::now(),
            None,  // No end date for ongoing subscriptions
            pricing_snapshot,
        ).await?;

        Ok(subscription)
    }

    /// Generate an invoice for a client for a specific billing period
    pub async fn generate_client_invoice(
        tx: &mut Transaction<'_, Postgres>,
        client_id: Uuid,
        application_id: Uuid,
        billing_period_id: Uuid,
    ) -> Result<ClientInvoice> {
        // Get the developer ID for this application
        let developer_id = sqlx::query_scalar!(
            r#"
            SELECT developer_id FROM applications WHERE id = $1
            "#,
            application_id
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| crate::error::VaultlessError::NotFound("Application not found".into()))?;

        // Get all usage for this client and application in the billing period
        let usage_records = ClientBillingUsage::find_by_client_and_period(
            &mut **tx,
            client_id,
            billing_period_id,
        ).await?;

        // Calculate total amount based on usage
        let total_amount_cents: i64 = usage_records.iter()
            .map(|usage| {
                // Extract cost from revenue snapshot or calculate from usage
                match &usage.revenue_snapshot {
                    serde_json::Value::Object(obj) => {
                        obj.get("usage_cost")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0)
                    },
                    _ => 0
                }
            })
            .sum();

        // Create pricing snapshot for the invoice
        let pricing_snapshot = serde_json::json!({
            "generated_at": Utc::now(),
            "usage_records_count": usage_records.len(),
            "subtotal_cents": total_amount_cents,
            "tax_cents": 0,  // Assuming no tax for simplicity
            "total_cents": total_amount_cents
        });

        // Create the invoice
        let invoice = ClientInvoice::create(
            tx,
            billing_period_id,
            client_id,
            application_id,
            developer_id,
            pricing_snapshot,
            total_amount_cents,  // subtotal
            total_amount_cents,  // total
            "pending".to_string(),  // status
        ).await?;

        Ok(invoice)
    }

    /// Check if a client has an active subscription to an application
    pub async fn check_client_entitlement(
        &self,
        pool: &PgPool,
        client_id: Uuid,
        application_id: Uuid,
    ) -> Result<bool> {
        // First check if the client has an active subscription to the application
        if let Some(_subscription) = ClientSubscription::find_by_client_and_application(
            pool,
            client_id,
            application_id,
        ).await? {
            return Ok(true);
        }

        // If no subscription, check if the client has sufficient credits for PAYG usage
        if let Some(credit) = ClientUsageCredit::find_by_client(pool, client_id).await? {
            // For now, we'll say any positive balance allows usage
            // In practice, this might be more nuanced based on expected usage
            return Ok(credit.credit_balance > 0);
        }

        // No subscription and no credits means no entitlement
        Ok(false)
    }
}