// vaultless-core/src/models/pricing/client_subscription.rs

use crate::cache_key;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use rust_decimal::prelude::FromStr;
use serde::{Deserialize, Serialize};
use sqlx::{types::Json, Executor, FromRow, Postgres};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::Result;

use super::{
    dto::CreateClientSubscription,
    enums::{PricingMode, SubscriptionStatus},
    snapshot::PricingSnapshot,
};

// =============================================================================
// CLIENT SUBSCRIPTION CACHE
// =============================================================================

/// Generate cache key for client subscription
#[inline]
pub fn client_subscription_cache_key(client_id: Uuid, application_id: Uuid) -> String {
    cache_key!("client_subscription", client_id, application_id)
}

/// Minimal cache-only struct for ClientSubscription Redis HASH storage.
/// Contains only fields from PricingSnapshot needed for hot-path validation.
#[derive(Debug, Clone)]
pub struct ClientSubscriptionCacheEntry {
    pub id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    // PricingSnapshot fields
    pub plan_id: Uuid,
    pub plan_name: String,
    pub pricing_mode: PricingMode,
    pub price_per_message_cents: Option<i64>,
    pub price_per_gb_cents: Option<i64>,
    pub price_per_proof_cents: Option<i64>,
    pub prepaid_amount_cents: Option<i64>,
    pub platform_fee_percent: Option<rust_decimal::Decimal>,
    pub currency: Option<String>,
    // Status
    pub is_active: bool,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// Redis field names for ClientSubscriptionCacheEntry HASH storage
pub mod client_sub_cache_field {
    pub const ID: &str = "id";
    pub const CLIENT_ID: &str = "client_id";
    pub const APPLICATION_ID: &str = "application_id";
    pub const PLAN_ID: &str = "plan_id";
    pub const PLAN_NAME: &str = "plan_name";
    pub const PRICING_MODE: &str = "pricing_mode";
    pub const PRICE_PER_MESSAGE: &str = "price_per_message";
    pub const PRICE_PER_GB: &str = "price_per_gb";
    pub const PRICE_PER_PROOF: &str = "price_per_proof";
    pub const PREPAID_AMOUNT: &str = "prepaid_amount";
    pub const PLATFORM_FEE_PERCENT: &str = "platform_fee_percent";
    pub const CURRENCY: &str = "currency";
    pub const IS_ACTIVE: &str = "is_active";
    pub const STARTED_AT: &str = "started_at";
    pub const ENDED_AT: &str = "ended_at";
}

impl ClientSubscriptionCacheEntry {
    /// Cache TTL (1 hour)
    pub const TTL_SECONDS: i64 = 3600;

    /// Convert from Redis HASH (HashMap<String, String>)
    /// Returns None if required fields are missing
    pub fn from_redis(vals: HashMap<String, String>) -> Option<Self> {
        Some(Self {
            id: vals.get(client_sub_cache_field::ID)?.parse().ok()?,
            client_id: vals.get(client_sub_cache_field::CLIENT_ID)?.parse().ok()?,
            application_id: vals.get(client_sub_cache_field::APPLICATION_ID)?.parse().ok()?,
            plan_id: vals.get(client_sub_cache_field::PLAN_ID)?.parse().ok()?,
            plan_name: vals.get(client_sub_cache_field::PLAN_NAME)?.clone(),
            pricing_mode: vals.get(client_sub_cache_field::PRICING_MODE)
                .map(|v| match v.as_str() {
                    "postpaid" => PricingMode::Postpaid,
                    "prepaid" => PricingMode::Prepaid,
                    _ => PricingMode::Free,
                })
                .unwrap_or(PricingMode::Free),
            price_per_message_cents: vals.get(client_sub_cache_field::PRICE_PER_MESSAGE).and_then(|v| v.parse().ok()),
            price_per_gb_cents: vals.get(client_sub_cache_field::PRICE_PER_GB).and_then(|v| v.parse().ok()),
            price_per_proof_cents: vals.get(client_sub_cache_field::PRICE_PER_PROOF).and_then(|v| v.parse().ok()),
            prepaid_amount_cents: vals.get(client_sub_cache_field::PREPAID_AMOUNT).and_then(|v| v.parse().ok()),
            platform_fee_percent: vals.get(client_sub_cache_field::PLATFORM_FEE_PERCENT)
                .and_then(|v| rust_decimal::Decimal::from_str(v).ok()),
            currency: vals.get(client_sub_cache_field::CURRENCY).cloned(),
            is_active: vals.get(client_sub_cache_field::IS_ACTIVE).map(|v| v == "1").unwrap_or(false),
            started_at: vals.get(client_sub_cache_field::STARTED_AT)?
                .parse()
                .ok()
                .or_else(|| Some(chrono::Utc::now()))?,
            ended_at: vals.get(client_sub_cache_field::ENDED_AT).and_then(|v| v.parse().ok()),
        })
    }

    /// Convert to Redis HASH compatible values for HMSET
    pub fn to_redis_args(&self) -> Vec<String> {
        let mut args = Vec::with_capacity(24);
        args.push(client_sub_cache_field::ID.to_string());
        args.push(self.id.to_string());
        args.push(client_sub_cache_field::CLIENT_ID.to_string());
        args.push(self.client_id.to_string());
        args.push(client_sub_cache_field::APPLICATION_ID.to_string());
        args.push(self.application_id.to_string());
        args.push(client_sub_cache_field::PLAN_ID.to_string());
        args.push(self.plan_id.to_string());
        args.push(client_sub_cache_field::PLAN_NAME.to_string());
        args.push(self.plan_name.clone());
        args.push(client_sub_cache_field::PRICING_MODE.to_string());
        args.push(match self.pricing_mode {
            PricingMode::Postpaid => "postpaid".to_string(),
            PricingMode::Prepaid => "prepaid".to_string(),
            PricingMode::Free => "free".to_string(),
        });
        args.push(client_sub_cache_field::PRICE_PER_MESSAGE.to_string());
        args.push(self.price_per_message_cents.map(|v| v.to_string()).unwrap_or_default());
        args.push(client_sub_cache_field::PRICE_PER_GB.to_string());
        args.push(self.price_per_gb_cents.map(|v| v.to_string()).unwrap_or_default());
        args.push(client_sub_cache_field::PRICE_PER_PROOF.to_string());
        args.push(self.price_per_proof_cents.map(|v| v.to_string()).unwrap_or_default());
        args.push(client_sub_cache_field::PREPAID_AMOUNT.to_string());
        args.push(self.prepaid_amount_cents.map(|v| v.to_string()).unwrap_or_default());
        args.push(client_sub_cache_field::PLATFORM_FEE_PERCENT.to_string());
        args.push(self.platform_fee_percent.map(|v| v.to_string()).unwrap_or_default());
        args.push(client_sub_cache_field::CURRENCY.to_string());
        args.push(self.currency.clone().unwrap_or_default());
        args.push(client_sub_cache_field::IS_ACTIVE.to_string());
        args.push(if self.is_active { "1".to_string() } else { "0".to_string() });
        args.push(client_sub_cache_field::STARTED_AT.to_string());
        args.push(self.started_at.to_rfc3339());
        args.push(client_sub_cache_field::ENDED_AT.to_string());
        args.push(self.ended_at.map(|v| v.to_rfc3339()).unwrap_or_default());
        args
    }

    /// Convert from ClientSubscription (Postgres result)
    /// Returns None if subscription is not active or ends in the future/now
    pub fn from_subscription(subscription: &ClientSubscription) -> Option<Self> {
        let now = Utc::now();

        // Don't cache if ended_at is >= now (subscription is effectively ended)
        if let Some(ended_at) = subscription.ended_at {
            if ended_at >= now {
                return None;
            }
        }

        // Don't cache if status is not Active
        if subscription.status != SubscriptionStatus::Active {
            return None;
        }

        let snapshot = &subscription.pricing_snapshot.0;
        Some(Self {
            id: subscription.id,
            client_id: subscription.client_id,
            application_id: subscription.application_id,
            plan_id: snapshot.plan_id,
            plan_name: snapshot.plan_name.clone(),
            pricing_mode: snapshot.pricing_mode,
            price_per_message_cents: snapshot.price_per_message_cents,
            price_per_gb_cents: snapshot.price_per_gb_cents,
            price_per_proof_cents: snapshot.price_per_proof_cents,
            prepaid_amount_cents: snapshot.prepaid_amount_cents,
            platform_fee_percent: snapshot.platform_fee_percent,
            currency: snapshot.currency.clone(),
            is_active: true,
            started_at: subscription.started_at,
            ended_at: subscription.ended_at,
        })
    }
}

// =============================================================================
// CLIENT SUBSCRIPTION
// =============================================================================

/// Client's subscription to an application's pricing plan
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ClientSubscription {
    pub id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub pricing_plan_id: Uuid,
    pub status: SubscriptionStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub pricing_snapshot: Json<PricingSnapshot>,
    pub created_at: DateTime<Utc>,
}

impl ClientSubscription {
    /// Check if the subscription is active
    pub fn is_active(&self) -> bool {
        self.status == SubscriptionStatus::Active && self.ended_at.is_none()
    }

    /// Check if the subscription is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.status == SubscriptionStatus::Cancelled
    }

    /// Create a new client subscription
    pub async fn create<'c, E>(executor: E, input: CreateClientSubscription, pricing_snapshot: PricingSnapshot) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Use CTE to atomically cancel old subscriptions and insert new one
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            WITH cancelled AS (
                UPDATE client_subscriptions
                SET status = 'cancelled', ended_at = NOW()
                WHERE client_id = $1 AND application_id = $2 AND status = 'active'
                RETURNING id
            )
            INSERT INTO client_subscriptions (
                client_id, application_id, pricing_plan_id, pricing_snapshot
            )
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(input.client_id)
        .bind(input.application_id)
        .bind(input.pricing_plan_id)
        .bind(Json(pricing_snapshot))
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }

    /// Find subscription by ID
    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| crate::error::VaultlessError::NotFound("Subscription not found".into()))
    }

    /// Get active subscription for a client and application
    pub async fn get_active<'c, E>(executor: E, client_id: Uuid, application_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM client_subscriptions
            WHERE client_id = $1 AND application_id = $2 AND status = 'active'
            "#,
        )
        .bind(client_id)
        .bind(application_id)
        .fetch_optional(executor)
        .await?;

        Ok(subscription)
    }

    /// Get active subscription with Redis caching.
    /// Uses cache-aside pattern: check Redis first, fall back to Postgres, then cache.
    pub async fn get_active_with_cache<'c, E>(
        executor: E,
        redis: Option<Arc<RedisPool>>,
        client_id: Uuid,
        application_id: Uuid,
    ) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = client_subscription_cache_key(client_id, application_id);

        // --- HOT PATH (Redis HASH) ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(vals) = conn.hgetall::<_, HashMap<String, String>>(&cache_key).await {
                    if !vals.is_empty() {
                        if let Some(cache_entry) = ClientSubscriptionCacheEntry::from_redis(vals) {
                            // Validate: subscription must be active and not ended
                            let now = Utc::now();
                            let is_valid = cache_entry.is_active
                                && cache_entry.ended_at.map(|e| e < now).unwrap_or(true);

                            if is_valid {
                                // Reconstruct full ClientSubscription from cache
                                return Ok(Some(ClientSubscription {
                                    id: cache_entry.id,
                                    client_id: cache_entry.client_id,
                                    application_id: cache_entry.application_id,
                                    pricing_plan_id: cache_entry.plan_id,
                                    status: SubscriptionStatus::Active,
                                    started_at: cache_entry.started_at,
                                    ended_at: cache_entry.ended_at,
                                    pricing_snapshot: Json(super::snapshot::PricingSnapshot {
                                        id: Uuid::new_v4(), // Generate a new UUID for the snapshot
                                        plan_id: cache_entry.plan_id,
                                        plan_name: cache_entry.plan_name,
                                        pricing_mode: cache_entry.pricing_mode,
                                        price_per_message_cents: cache_entry.price_per_message_cents,
                                        price_per_gb_cents: cache_entry.price_per_gb_cents,
                                        price_per_proof_cents: cache_entry.price_per_proof_cents,
                                        prepaid_amount_cents: cache_entry.prepaid_amount_cents,
                                        platform_fee_percent: cache_entry.platform_fee_percent,
                                        currency: cache_entry.currency,
                                    }),
                                    created_at: chrono::Utc::now(),
                                }));
                            }
                            // Cache exists but subscription is inactive
                            return Ok(None);
                        }
                    }
                }
            }
        }

        // --- POSTGRES FALLBACK ---
        let subscription = Self::get_active(executor, client_id, application_id).await?;

        // --- CACHE-ASIDE: Update Redis if active subscription found ---
        if let (Some(sub), Some(redis_pool)) = (&subscription, redis) {
            if sub.is_active() {
                if let Some(cache_entry) = ClientSubscriptionCacheEntry::from_subscription(sub) {
                    if let Ok(mut conn) = redis_pool.get().await {
                        let args = cache_entry.to_redis_args();
                        let mut cmd = redis::cmd("HMSET");
                        cmd.arg(&cache_key);
                        for arg in &args {
                            cmd.arg(arg);
                        }
                        let _: () = cmd.query_async(&mut conn).await?;
                        let _: () = redis::cmd("EXPIRE")
                            .arg(&cache_key)
                            .arg(ClientSubscriptionCacheEntry::TTL_SECONDS)
                            .query_async(&mut conn)
                            .await?;
                    }
                }
            }
        }

        Ok(subscription)
    }

    /// Get all subscriptions for a client
    pub async fn find_by_client<'c, E>(executor: E, client_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscriptions = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE client_id = $1 ORDER BY started_at DESC",
        )
        .bind(client_id)
        .fetch_all(executor)
        .await?;

        Ok(subscriptions)
    }

    /// Get all subscriptions for an application
    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscriptions = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE application_id = $1 ORDER BY started_at DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(subscriptions)
    }

    /// Update subscription status
    pub async fn update_status<'c, E>(executor: E, id: Uuid, status: SubscriptionStatus) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            UPDATE client_subscriptions
            SET status = $1, ended_at = CASE WHEN $1 = 'cancelled' THEN NOW() ELSE ended_at END
            WHERE id = $2
            RETURNING *
            "#,
        )
        .bind(status)
        .bind(id)
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }

    /// Cancel subscription
    pub async fn cancel<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, SubscriptionStatus::Cancelled).await
    }

    /// Pause subscription
    pub async fn pause<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, SubscriptionStatus::Paused).await
    }

    /// Resume subscription
    pub async fn resume<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, SubscriptionStatus::Active).await
    }
}
