// Ensure Application, VaultlessError, etc. are imported
use super::dto::{Application, IntegrityConfig, UpdateIntegrityConfigRequest};
use crate::error::{Result, VaultlessError};
use sqlx::{Executor, Postgres};
use validator::Validate;

impl Application {
    // Helper function (assuming it's here or accessible)
    pub fn set_integrity_config(config: IntegrityConfig) -> Result<serde_json::Value> {
        serde_json::to_value(config).map_err(|e| {
            VaultlessError::Serialization(format!("Failed to serialize integrity config: {}", e))
        })
    }

    /// Standardized method to update and persist the entire integrity configuration.
    pub async fn configure_integrity<'c, E>(
        &self,
        exec: E,
        input: UpdateIntegrityConfigRequest,
    ) -> Result<IntegrityConfig>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // 1. Validate the entire configuration input.
        input.validate().map_err(|e| {
            VaultlessError::Validation(format!("Integrity configuration failed validation: {}", e))
        })?;

        // 2. Convert the validated input DTO into the final IntegrityConfig struct.
        let updated_config = IntegrityConfig {
            web: input.web,
            ios: input.ios,
            android: input.android,
        };

        // 3. Serialize the updated config back to JSONB Value.
        let new_jsonb = Self::set_integrity_config(updated_config.clone())?;

        // 4. Persist the new JSONB value to the database.
        sqlx::query(
            r#"
            UPDATE applications
            SET integrity_config = $1,
                updated_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(&new_jsonb)
        .bind(self.id)
        .execute(exec)
        .await
        .map_err(VaultlessError::Database)?;

        // 5. Return the successfully updated configuration.
        Ok(updated_config)
    }

    /// Sets and persists the default configuration for application integrity.
    pub async fn set_default_integrity_config<'c, E>(&self, exec: E) -> Result<IntegrityConfig>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Use the derived Default trait for initialization.
        let default_config = IntegrityConfig::default();

        // Serialize the default config.
        let new_jsonb = Self::set_integrity_config(default_config.clone())?;

        // Persist the default JSONB value to the database.
        sqlx::query(
            r#"
        UPDATE applications
        SET integrity_config = $1,
            updated_at = NOW()
        WHERE id = $2
        "#,
        )
        .bind(&new_jsonb)
        .bind(self.id)
        .execute(exec)
        .await
        .map_err(VaultlessError::Database)?;

        Ok(default_config)
    }
}
