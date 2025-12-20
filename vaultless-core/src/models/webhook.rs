//! Webhook management module for Vaultless applications.
//!
//! Webhooks allow applications to receive real-time notifications about events.

use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgExecutor, Postgres, Transaction};
use uuid::Uuid;

use super::app_model::dto::{WebhookEventType, WebhookInput};

/// Database model for webhooks table
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WebhookRecord {
    pub id: Uuid,
    pub application_id: Uuid,
    pub url: String,
    /// Stored as string in DB, converted to/from WebhookEventType
    pub event_type: String,
    pub signing_secret: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WebhookRecord {
    /// Get the event type as an enum
    pub fn event_type_enum(&self) -> Result<WebhookEventType> {
        self.event_type.parse().map_err(|e: String| VaultlessError::Validation(e))
    }
}

impl WebhookRecord {
    /// Generates a new signing secret for webhook payload verification
    fn generate_signing_secret() -> String {
        use rand::Rng;
        let secret: [u8; 32] = rand::rng().random();
        format!("whsec_{}", hex::encode(secret))
    }

    /// Creates a new webhook for an application
    pub async fn create<'e, E>(
        executor: E,
        application_id: Uuid,
        input: &WebhookInput,
    ) -> Result<Self>
    where
        E: PgExecutor<'e>,
    {
        let signing_secret = Self::generate_signing_secret();
        let event_type_str = input.event_type.as_str();

        let webhook = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO webhooks (application_id, url, event_type, signing_secret, is_active)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(application_id)
        .bind(&input.url)
        .bind(event_type_str)
        .bind(&signing_secret)
        .bind(input.is_active)
        .fetch_one(executor)
        .await?;

        tracing::info!(
            webhook_id = %webhook.id,
            application_id = %application_id,
            url = %input.url,
            event_type = %input.event_type,
            "Webhook created"
        );

        Ok(webhook)
    }

    /// Updates an existing webhook
    pub async fn update<'e, E>(
        executor: E,
        webhook_id: Uuid,
        application_id: Uuid,
        input: &WebhookInput,
    ) -> Result<Self>
    where
        E: PgExecutor<'e>,
    {
        let event_type_str = input.event_type.as_str();

        let webhook = sqlx::query_as::<_, Self>(
            r#"
            UPDATE webhooks
            SET url = $1, event_type = $2, is_active = $3, updated_at = NOW()
            WHERE id = $4 AND application_id = $5
            RETURNING *
            "#,
        )
        .bind(&input.url)
        .bind(event_type_str)
        .bind(input.is_active)
        .bind(webhook_id)
        .bind(application_id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound(format!(
                "Webhook {} not found for application {}",
                webhook_id, application_id
            ))
        })?;

        tracing::info!(
            webhook_id = %webhook_id,
            application_id = %application_id,
            "Webhook updated"
        );

        Ok(webhook)
    }

    /// Deletes webhooks by IDs for an application
    pub async fn delete_by_ids<'e, E>(
        executor: E,
        application_id: Uuid,
        webhook_ids: &[Uuid],
    ) -> Result<u64>
    where
        E: PgExecutor<'e>,
    {
        if webhook_ids.is_empty() {
            return Ok(0);
        }

        let result = sqlx::query(
            r#"
            DELETE FROM webhooks
            WHERE application_id = $1 AND id = ANY($2)
            "#,
        )
        .bind(application_id)
        .bind(webhook_ids)
        .execute(executor)
        .await?;

        let deleted = result.rows_affected();

        tracing::info!(
            application_id = %application_id,
            deleted_count = deleted,
            "Webhooks deleted"
        );

        Ok(deleted)
    }

    /// Gets all webhook IDs for an application
    pub async fn get_ids_for_application<'e, E>(
        executor: E,
        application_id: Uuid,
    ) -> Result<Vec<Uuid>>
    where
        E: PgExecutor<'e>,
    {
        let ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM webhooks WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(ids)
    }

    /// Lists all webhooks for an application
    pub async fn list_for_application<'e, E>(
        executor: E,
        application_id: Uuid,
    ) -> Result<Vec<Self>>
    where
        E: PgExecutor<'e>,
    {
        let webhooks = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM webhooks
            WHERE application_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(webhooks)
    }

    /// Synchronizes webhooks for an application based on the input list.
    ///
    /// - Creates webhooks with `id: None`
    /// - Updates webhooks with `id: Some(uuid)`
    /// - Deletes webhooks not in the input list
    pub async fn sync_webhooks(
        tx: &mut Transaction<'_, Postgres>,
        application_id: Uuid,
        webhooks: &[WebhookInput],
    ) -> Result<Vec<Self>> {
        // Get existing webhook IDs
        let existing_ids = Self::get_ids_for_application(&mut **tx, application_id).await?;

        // Determine which webhooks to update (have IDs) and which to create (no IDs)
        let mut input_ids: Vec<Uuid> = Vec::new();
        let mut to_create: Vec<&WebhookInput> = Vec::new();
        let mut to_update: Vec<(Uuid, &WebhookInput)> = Vec::new();

        for webhook in webhooks {
            if let Some(id) = webhook.id {
                input_ids.push(id);
                to_update.push((id, webhook));
            } else {
                to_create.push(webhook);
            }
        }

        // Find webhooks to delete (exist in DB but not in input)
        let to_delete: Vec<Uuid> = existing_ids
            .iter()
            .filter(|id| !input_ids.contains(id))
            .copied()
            .collect();

        // Capture counts before moving vectors
        let created_count = to_create.len();
        let updated_count = to_update.len();
        let deleted_count = to_delete.len();

        // Delete removed webhooks
        if !to_delete.is_empty() {
            Self::delete_by_ids(&mut **tx, application_id, &to_delete).await?;
        }

        // Update existing webhooks
        let mut results = Vec::new();
        for (id, input) in to_update {
            // Verify the webhook ID actually exists for this application
            if existing_ids.contains(&id) {
                let updated = Self::update(&mut **tx, id, application_id, input).await?;
                results.push(updated);
            } else {
                return Err(VaultlessError::NotFound(format!(
                    "Webhook {} not found for application {}",
                    id, application_id
                )));
            }
        }

        // Create new webhooks
        for input in to_create {
            let created = Self::create(&mut **tx, application_id, input).await?;
            results.push(created);
        }

        // Sort by created_at for consistent ordering
        results.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        tracing::info!(
            application_id = %application_id,
            created = created_count,
            updated = updated_count,
            deleted = deleted_count,
            "Webhooks synchronized"
        );

        Ok(results)
    }

    /// Regenerates the signing secret for a webhook
    pub async fn regenerate_signing_secret<'e, E>(
        executor: E,
        webhook_id: Uuid,
        application_id: Uuid,
    ) -> Result<Self>
    where
        E: PgExecutor<'e>,
    {
        let new_secret = Self::generate_signing_secret();

        let webhook = sqlx::query_as::<_, Self>(
            r#"
            UPDATE webhooks
            SET signing_secret = $1, updated_at = NOW()
            WHERE id = $2 AND application_id = $3
            RETURNING *
            "#,
        )
        .bind(&new_secret)
        .bind(webhook_id)
        .bind(application_id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound(format!(
                "Webhook {} not found for application {}",
                webhook_id, application_id
            ))
        })?;

        tracing::info!(
            webhook_id = %webhook_id,
            application_id = %application_id,
            "Webhook signing secret regenerated"
        );

        Ok(webhook)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_signing_secret() {
        let secret = WebhookRecord::generate_signing_secret();
        assert!(secret.starts_with("whsec_"));
        assert_eq!(secret.len(), 70); // "whsec_" (6) + 64 hex chars
    }
}
