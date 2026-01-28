use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::Result,
    models::{
        billing::{
            ClientUsageCredit, CreditTransaction, DeveloperRevenueShare, PspAccount, PspPayout
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
    ) -> Result<()> {
        // Calculate usage totals
        let total_messages = messages_sent + messages_received;
        let total_bytes = bytes_sent + bytes_received;

        // Calculate required credits (these are non-cash usage units)
        let required_credits = Self::calculate_required_credits(
            total_messages,
            total_bytes,
            proofs_verified,
            pricing_snapshot,
        );

        // Check if client has sufficient credits
        let mut client_credit = ClientUsageCredit::find_by_client(
            &mut **tx,
            client_id,
        )
        .await?
        .ok_or_else(|| {
            crate::error::VaultlessError::NotFound(
                "Client usage credit record not found".into(),
            )
        })?;

        if client_credit.credit_balance < required_credits {
            return Err(crate::error::VaultlessError::InsufficientCredits(
                "Insufficient credits for this usage".to_string(),
            ));
        }

        // Deduct credits from client (these are non-cash units)
        ClientUsageCredit::update_balance(
            tx,
            client_id,
            -required_credits, // Negative to decrease balance
        )
        .await?;

        // Record the credit deduction transaction
        // Using USD as default currency, but this should come from the client's payment context
        CreditTransaction::create(
            tx,
            client_id,
            application_id,
            "usage_deduction".to_string(),
            -required_credits, // Negative to indicate deduction
            "USD".to_string(), // Default currency
            0, // Credit unit value - for usage deductions, this might be 0 or derived differently
            Some(serde_json::json!({
                "messages_sent": messages_sent,
                "messages_received": messages_received,
                "bytes_sent": bytes_sent,
                "bytes_received": bytes_received,
                "proofs_verified": proofs_verified,
                "pricing_snapshot": pricing_snapshot
            })),
            None, // No related transaction for usage deduction
            Some(billing_period_id),
        )
        .await?;

        // Calculate revenue for the developer based on usage (this is accounting metadata only)
        let gross_revenue_cents = Self::calculate_gross_revenue_cents(
            total_messages,
            total_bytes,
            proofs_verified,
            pricing_snapshot,
        );

        // Platform takes a percentage (configurable per application or globally)
        let platform_fee_percent = Decimal::new(1000, 3); // 10.00% as example
        let platform_fee_cents = (gross_revenue_cents as f64 * platform_fee_percent.to_f64().unwrap_or(0.1)) as i64;
        let net_revenue_cents = gross_revenue_cents - platform_fee_cents;

        // Create revenue share record (this is accounting metadata, not real money held by platform)
        DeveloperRevenueShare::create(
            tx,
            developer_id,
            application_id,
            billing_period_id,
            total_messages,
            total_bytes,
            proofs_verified,
            gross_revenue_cents,
            platform_fee_percent,
            platform_fee_cents,
            net_revenue_cents,
        )
        .await?;

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
        ClientUsageCredit::update_balance(
            tx,
            client_id,
            credits_to_add, // Positive to increase balance
        )
        .await?;

        // Record the credit addition transaction
        // For credit additions, we can use a special application ID or a system application
        // For now, we'll use a system application ID or nil if it's a general credit addition
        CreditTransaction::create(
            tx,
            client_id,
            application_id, // The application context where the credits might be used
            "credit_purchase".to_string(), // Or "credit_allocation" depending on source
            credits_to_add, // Positive to indicate addition
            "USD".to_string(), // Currency code - should come from payment context
            cash_value_cents / credits_to_add.max(1), // Credit unit value (avoid division by zero)
            Some(serde_json::json!({
                "action": "credit_addition",
                "credits_added": credits_to_add,
                "cash_value_cents": cash_value_cents  // Keeping for reference but not used for accounting
            })),
            None,
            None, // No billing period for credit addition
        )
        .await?;

        Ok(())
    }

    /// Process payouts to developers at the end of a billing period
    /// This function requests the PSP to move money - platform never holds funds
    pub async fn process_payouts_for_billing_period(
        pool: &PgPool,
        billing_period_id: Uuid,
        platform_fee_percent: Decimal,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;

        // Get all revenue shares for this billing period
        let revenue_shares = DeveloperRevenueShare::find_by_billing_period(&mut *tx, billing_period_id).await?;

        // Group by developer to create consolidated payouts
        let mut developer_payouts: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();
        
        for share in &revenue_shares {
            let current_amount = developer_payouts.entry(share.developer_id).or_insert(0);
            *current_amount += share.net_revenue_cents; // Add the developer's net revenue
        }

        // Process each developer's payout
        for (developer_id, total_payout_amount) in developer_payouts {
            // Get the developer's PSP account
            let psp_account = PspAccount::find_by_developer(&mut *tx, developer_id).await?
                .ok_or_else(|| crate::error::VaultlessError::NotFound(
                    format!("PSP account not found for developer {}", developer_id)
                ))?;

            // Get developer's preferred payout currency (defaulting to USD if not specified)
            let developer_currency = Self::get_developer_payout_currency(&mut *tx, developer_id).await?;

            // Create a payout record with currency conversion details (platform never holds these funds)
            let payout = PspPayout::create(
                &mut tx,
                developer_id,
                psp_account.id,
                total_payout_amount, // amount_cents - kept for backward compatibility
                "USD".to_string(), // currency - kept for backward compatibility
                "USD".to_string(), // source_currency - where funds are settled
                developer_currency, // destination_currency - developer's preferred currency
                total_payout_amount, // requested_amount in source currency
                total_payout_amount, // converted_amount (initially same, updated after FX conversion)
                None, // fx_rate - will be set during actual FX conversion
            ).await?;

            // In a real implementation, we would call the PSP API here to initiate the payout
            // For now, we'll simulate the PSP request
            Self::request_psp_payout(&mut tx, payout.id).await?;
        }

        // Update billing period to indicate PSP processing is complete
        sqlx::query!(
            r#"
            UPDATE billing_periods 
            SET psp_processing_status = 'completed', 
                psp_processing_completed_at = NOW(),
                platform_revenue_cents = $2
            WHERE id = $1
            "#,
            billing_period_id,
            // Calculate platform revenue as sum of all platform fees
            // This would be computed from the revenue shares
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Get developer's preferred payout currency
    /// In a real implementation, this would come from developer's profile/settings
    async fn get_developer_payout_currency(tx: &mut Transaction<'_, Postgres>, developer_id: Uuid) -> Result<String> {
        // In a real implementation, this would fetch from a developer preferences table
        // For now, default to USD but could be different based on developer's region/country
        let currency = sqlx::query_scalar!(
            r#"
            SELECT COALESCE(preferred_payout_currency, 'USD') as currency
            FROM users
            WHERE id = $1
            "#,
            developer_id
        )
        .fetch_one(tx)
        .await
        .unwrap_or_else(|_| "USD".to_string());

        Ok(currency)
    }

    /// Simulate requesting a payout from the PSP
    /// In reality, this would make an HTTP call to the PSP's API
    async fn request_psp_payout(tx: &mut Transaction<'_, Postgres>, payout_id: Uuid) -> Result<()> {
        // In a real implementation, this would:
        // 1. Look up the payout details
        let payout = PspPayout::find_by_id(&mut **tx, payout_id).await?;

        // 2. Handle currency conversion if source != destination
        let (converted_amount, fx_rate) = if payout.source_currency != payout.destination_currency {
            // In real implementation, fetch current FX rate from provider
            // For simulation, assume 1:1 rate
            (payout.requested_amount, Some(rust_decimal::Decimal::ONE))
        } else {
            (payout.requested_amount, None)
        };

        // 3. Call the PSP API to initiate the payout
        // 4. Store the PSP's response (transaction ID, status, etc.)
        // 5. Update the payout record with PSP details

        // For simulation purposes, we'll just update the status to "processing"
        PspPayout::update_payout_status(
            &mut **tx,
            payout_id,
            Some(format!("psp_payout_{}", payout_id)), // Simulated PSP payout ID
            Some("initiated".to_string()),
            Some(serde_json::json!({"status": "initiated", "message": "Payout initiated with PSP"})),
            "processing".to_string(),
            Some(Utc::now()),
            None, // Not delivered yet
            None,
            None, // psp_fee_deducted
            Some(converted_amount), // net_paid_amount
            None, // settlement_date
            Some(serde_json::json!({
                "psp_payout_id": format!("psp_payout_{}", payout_id),
                "status": "initiated",
                "paid_amount": converted_amount,
                "currency": payout.destination_currency,
                "fee_deducted": 0,
                "settlement_date": Utc::now() + chrono::Duration::days(2) // Simulated settlement date
            })), // psp_normalized_response
        )
        .await?;

        Ok(())
    }

    /// Calculate required credits based on usage and pricing
    /// These are non-cash usage units, not real money
    fn calculate_required_credits(
        total_messages: i64,
        total_bytes: i64,
        proofs_verified: i64,
        pricing: &PricingSnapshot,
    ) -> i64 {
        // Calculate credits needed for messages
        let message_credits = match pricing.price_per_message_cents {
            Some(price_per_message) if price_per_message > 0 => {
                // Convert price per message to credits needed
                // For example, if 1000 credits = $10.00, then each cent = 10 credits
                let credits_per_cent = 10; // Example: 1000 credits = $10.00 = 1000 cents, so 1 cent = 10 credits
                total_messages * price_per_message * credits_per_cent / 100
            }
            _ => 0,
        };

        // Calculate credits needed for bytes
        let byte_credits = match pricing.price_per_gb_cents {
            Some(price_per_gb) if price_per_gb > 0 => {
                // Convert bytes to GB and calculate credits
                let gb_transferred = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                let credits_per_cent = 10; // Same example ratio
                (gb_transferred * price_per_gb as f64 * credits_per_cent as f64 / 100.0) as i64
            }
            _ => 0,
        };

        // Calculate credits needed for proofs
        let proof_credits = match pricing.price_per_proof_cents {
            Some(price_per_proof) if price_per_proof > 0 => {
                proofs_verified * price_per_proof
            }
            _ => 0,
        };

        message_credits + byte_credits + proof_credits
    }


    /// Calculate gross revenue in cents based on usage
    /// This is accounting metadata only, not real money held by platform
    fn calculate_gross_revenue_cents(
        total_messages: i64,
        total_bytes: i64,
        proofs_verified: i64,
        pricing: &PricingSnapshot,
    ) -> i64 {
        // Calculate revenue for messages
        let message_revenue = match pricing.price_per_message_cents {
            Some(price_per_message) => total_messages * price_per_message,
            None => 0,
        };

        // Calculate revenue for bytes
        let byte_revenue = match pricing.price_per_gb_cents {
            Some(price_per_gb) => {
                let gb_transferred = total_bytes / (1024 * 1024 * 1024);
                gb_transferred * price_per_gb
            },
            None => 0,
        };

        // Calculate revenue for proofs
        let proof_revenue = match pricing.price_per_proof_cents {
            Some(price_per_proof) => proofs_verified * price_per_proof,
            None => 0,
        };

        message_revenue + byte_revenue + proof_revenue
    }
}