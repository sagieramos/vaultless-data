use super::dto::*;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;
use crate::types::SubscriptionTier;
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;

impl Application {
    /// Invalidate application cache (standard method - requires full Application)
    /// Fetches the Application, its associated Secret Key data, and the Publishable
    /// Key plaintext in a single JOIN query, using the Application ID as the source.
    ///
    /// This method is intended to be called *after* a Secret Key ID has been resolved
    /// from a plaintext key (SK or PK).
    pub async fn find_with_tier<'c, E>(exec: E, app_id: Uuid) -> Result<ApplicationWithTier>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // The query performs two LEFT JOINs on the api_keys table:
        // 1. Join for the Secret Key (using applications.secret_key_id) to get tier/quota info.
        // 2. Join for the Publishable Key (using api_keys.application_id and key_type='publishable')
        //    to get the publishable_key_plaintext.
        let app_with_tier = sqlx::query_as::<_, ApplicationWithTier>(
            r#"
            SELECT
                a.id, a.user_id, a.name, a.description, a.secret_key_id,
                a.bundle_id, a.platform, a.webhook_url, a.is_active,
                a.created_at, a.updated_at,

                -- Data from Secret Key (ak_secret)
                ak_secret.tier,
                ak_secret.monthly_message_quota,
                ak_secret.rate_limit_per_minute,
                ak_secret.message_retention_seconds,
                ak_secret.is_active AS api_key_active,

                -- Data from Publishable Key (ak_publishable)
                ak_publishable.publishable_key_plaintext
            FROM
                applications a
            -- 1. JOIN for Secret Key data (Tier/Quota)
            JOIN
                api_keys ak_secret ON ak_secret.id = a.secret_key_id
            -- 2. JOIN for Publishable Key plaintext
            JOIN
                api_keys ak_publishable ON ak_publishable.application_id = a.id
                AND ak_publishable.key_type = 'publishable'
            WHERE
                a.id = $1
            -- Ensure a complete bundle is returned (optional, but good practice)
            AND ak_secret.key_type = 'secret'
        "#,
        )
        .bind(app_id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Application with tier data not found.".to_string())
        })?;

        Ok(app_with_tier)
    }

    pub async fn get_validation_cache<'c, E>(
        exec: E,
        secret_key_id: Uuid,
    ) -> Result<ApplicationValidationCache>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // The JOIN query fetches all required data in a single database call.
        let record = sqlx::query!(
            r#"
            SELECT
                a.id AS application_id,
                a.user_id,
                a.secret_key_id,
                a.is_active AS is_active,
                ak.is_active AS api_key_active,
                ak.expires_at AS api_key_expires_at,
                ak.tier AS "tier: SubscriptionTier",
                ak.monthly_message_quota,
                ak.rate_limit_per_minute
            FROM
                applications a
            JOIN
                api_keys ak ON ak.id = a.secret_key_id
            WHERE
                a.secret_key_id = $1
        "#,
            secret_key_id
        )
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Application data not found for validation.".to_string())
        })?;

        // Map the record fields to the final DTO
        let now = Utc::now();
        let api_key_expired = record
            .api_key_expires_at
            .map_or(false, |expiry| expiry < now);

        Ok(ApplicationValidationCache {
            application_id: record.application_id,
            user_id: record.user_id,
            secret_key_id: record.secret_key_id,
            is_active: record.is_active,

            api_key_active: record.api_key_active,
            api_key_expired,
            api_key_expires_at: record.api_key_expires_at,

            tier: record.tier.unwrap_or(SubscriptionTier::Free),

            monthly_quota_limit: record.monthly_message_quota.unwrap_or(0),
            rate_limit_per_minute: record.rate_limit_per_minute.unwrap_or(0),
        })
    }
}
