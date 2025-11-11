use super::dto::*;
use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;
use crate::types::SubscriptionTier;
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;

impl Application {
    /// Comprehensive validation: active status, tier limits, and secret key health
    pub async fn validate<'c, E>(
        &self,
        exec: E,
        redis: Option<Arc<RedisPool>>,
    ) -> Result<ApplicationValidation>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let mut validation = ApplicationValidation {
            is_valid: true,
            is_active: self.is_active,
            api_key_active: true,
            tier: None,
            quota_status: None,
            errors: Vec::new(),
        };

        // 1. Check if application is active
        if !self.is_active {
            validation.is_valid = false;
            validation.errors.push(ValidationError {
                code: "APPLICATION_INACTIVE".to_string(),
                message: "Application is deactivated".to_string(),
                severity: ErrorSeverity::Critical,
            });
            return Ok(validation); // Early return
        }

        // 2. Get and validate the secret API key
        let api_key =
            match ApiKey::find_by_id(exec.clone(), redis.clone(), self.secret_key_id).await {
                Ok(key) => key,
                Err(e) => {
                    validation.is_valid = false;
                    validation.errors.push(ValidationError {
                        code: "SECRET_KEY_NOT_FOUND".to_string(),
                        message: format!("Secret key not found: {}", e),
                        severity: ErrorSeverity::Critical,
                    });
                    return Ok(validation);
                }
            };

        validation.tier = Some(api_key.tier);

        // 3. Check if API key is active
        if !api_key.is_active {
            validation.is_valid = false;
            validation.api_key_active = false;
            validation.errors.push(ValidationError {
                code: "API_KEY_INACTIVE".to_string(),
                message: "API key is deactivated".to_string(),
                severity: ErrorSeverity::Critical,
            });
        }

        // 4. Check API key expiry
        if let Some(expires_at) = api_key.expires_at {
            if expires_at < Utc::now() {
                validation.is_valid = false;
                validation.errors.push(ValidationError {
                    code: "API_KEY_EXPIRED".to_string(),
                    message: format!("API key expired at {}", expires_at),
                    severity: ErrorSeverity::Critical,
                });
            }
        }

        // 5. Check quota (soft validation - doesn't fail, just warns)
        let quota = api_key.monthly_message_quota as i64;
        match ApiKey::check_quota(exec.clone(), redis.clone(), self.secret_key_id, quota).await {
            Ok(is_allowed) => {
                let current_usage =
                    Self::get_current_usage(exec, redis.clone(), self.secret_key_id).await?;

                validation.quota_status = Some(QuotaStatus {
                    limit: quota,
                    used: current_usage,
                    remaining: quota.saturating_sub(current_usage),
                    percentage_used: (current_usage as f64 / quota as f64 * 100.0).min(100.0),
                    is_exceeded: !is_allowed,
                });

                if !is_allowed {
                    validation.is_valid = false;
                    validation.errors.push(ValidationError {
                        code: "QUOTA_EXCEEDED".to_string(),
                        message: format!(
                            "Monthly quota exceeded: {}/{} messages used",
                            current_usage, quota
                        ),
                        severity: ErrorSeverity::Critical,
                    });
                } else if current_usage as f64 / quota as f64 > 0.9 {
                    // Warning at 90% usage
                    validation.errors.push(ValidationError {
                        code: "QUOTA_WARNING".to_string(),
                        message: format!(
                            "Approaching quota limit: {}/{} messages used ({}%)",
                            current_usage,
                            quota,
                            (current_usage as f64 / quota as f64 * 100.0).round()
                        ),
                        severity: ErrorSeverity::Warning,
                    });
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check quota for application {}: {}", self.id, e);
                validation.errors.push(ValidationError {
                    code: "QUOTA_CHECK_FAILED".to_string(),
                    message: "Unable to verify quota status".to_string(),
                    severity: ErrorSeverity::Warning,
                });
            }
        }

        // 6. Tier-specific validations
        match api_key.tier {
            SubscriptionTier::Free => {
                if let Some(ref quota_status) = validation.quota_status {
                    if quota_status.used > 500 && quota_status.percentage_used < 100.0 {
                        validation.errors.push(ValidationError {
                            code: "FREE_TIER_LIMIT_WARNING".to_string(),
                            message: "Consider upgrading to Pro for higher limits".to_string(),
                            severity: ErrorSeverity::Info,
                        });
                    }
                }
            }
            SubscriptionTier::Starter | SubscriptionTier::Pro | SubscriptionTier::Enterprise => {
                // Paid tiers - all good
            }
        }

        Ok(validation)
    }

    /// Quick validation - returns Result (for middleware use)
    pub async fn validate_or_error<'c, E>(
        &self,
        exec: E,
        redis: Option<Arc<RedisPool>>,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let validation = self.validate(exec, redis).await?;

        if !validation.is_valid {
            // Get the first critical error
            let error = validation
                .errors
                .iter()
                .find(|e| e.severity == ErrorSeverity::Critical)
                .or_else(|| validation.errors.first())
                .ok_or_else(|| {
                    VaultlessError::Internal("Validation failed without error details".into())
                })?;

            return Err(match error.code.as_str() {
                "APPLICATION_INACTIVE" => VaultlessError::Unauthorized(error.message.clone()),
                "API_KEY_INACTIVE" => VaultlessError::ApiKeyInactive,
                "API_KEY_EXPIRED" => VaultlessError::ApiKeyExpired,
                "QUOTA_EXCEEDED" => VaultlessError::QuotaExceeded(error.message.clone()),
                _ => VaultlessError::Unauthorized(error.message.clone()),
            });
        }

        Ok(())
    }

    /// Check if application can accept new clients
    pub async fn can_accept_new_clients<'c, E>(
        &self,
        exec: E,
        redis: Option<Arc<RedisPool>>,
    ) -> Result<bool>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let validation = self.validate(exec, redis).await?;

        Ok(validation.is_valid
            && validation
                .quota_status
                .map(|q| !q.is_exceeded)
                .unwrap_or(true))
    }

    /// Get application health summary
    pub async fn health_check<'c, E>(
        &self,
        exec: E,
        redis: Option<Arc<RedisPool>>,
    ) -> Result<ApplicationHealth>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let validation = self.validate(exec, redis).await?;

        let status = if validation.is_valid {
            if validation.errors.is_empty() {
                HealthStatus::Healthy
            } else {
                HealthStatus::Warning
            }
        } else {
            HealthStatus::Unhealthy
        };

        Ok(ApplicationHealth {
            status,
            application_id: self.id,
            is_active: validation.is_active,
            tier: validation.tier,
            quota: validation.quota_status,
            issues: validation.errors,
            checked_at: Utc::now(),
        })
    }

    async fn get_current_usage<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        api_key_id: Uuid,
    ) -> Result<i64>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        ApiKey::get_monthly_usage(exec, redis, api_key_id).await
    }
}
