use super::dto::*;
use crate::error::{Result, VaultlessError};
use sqlx::{Executor, Postgres};
use uuid::Uuid;

impl Application {
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
                a.authorized_origin, -- <-- NEW
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
}
