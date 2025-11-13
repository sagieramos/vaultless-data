use super::dto::*;
use crate::error::{Result, VaultlessError}; // Use our unified Result and VaultlessError
use crate::models::{
    ApiKey, CachedApiKey,
    usage::{MetricGranularity, MetricKey, MetricsConfig, increment_rate_limit_hit_pool},
};
use chrono::Utc;
use serde_json::Value; // Needed for JSONB/integrity_config handling
use sqlx::{Executor, Postgres};
use std::collections::HashMap;
use std::sync::Arc;


impl CachedResolvedKeyBundle {
    /// Hot-path optimized validation
    pub async fn validate_hot(
        &self,
        redis_pool: Arc<deadpool_redis::Pool>,
        request_headers: &HashMap<String, String>,
    ) -> Result<()> {
        // <-- Updated signature to use VaultlessError::Result<()>
        let sk = &self.secret_key_row;
        let app = &self.application;

        // 1. In-memory fast checks
        if !sk.is_active {
            return Err(VaultlessError::ApiKeyInactive);
        }
        if !app.is_active {
            return Err(VaultlessError::Forbidden(
                "Associated application is deactivated.".into(),
            ));
        }

        if let Some(expiry) = sk.expires_at {
            if Utc::now() > expiry {
                return Err(VaultlessError::ApiKeyExpired);
            }
        }

        // 2. PLATFORM INTEGRITY CHECK
        if let Some(platform) = &app.platform {
            match platform.as_str() {
                "web" => {
                    // WEB INTEGRITY: Check Origin against integrity_config
                    if let Some(origin) = request_headers.get("Origin") {
                        if let Some(Value::Array(allowed_origins)) =
                            app.integrity_config.get("authorized_origins")
                        {
                            let is_allowed = allowed_origins
                                .iter()
                                .any(|v| v.as_str().map_or(false, |s| s == origin));

                            if !is_allowed {
                                return Err(VaultlessError::IntegrityCheckFailed(format!(
                                    "Origin '{}' is not authorized for this web application.",
                                    origin
                                )));
                            }
                        } else {
                            // Fail open if web platform but config is missing/empty, but log a warning.
                            tracing::warn!(app_id = %app.id, "Web application is missing 'authorized_origins' config. Failing open.");
                        }
                    } else {
                        // Missing Origin header
                        return Err(VaultlessError::IntegrityCheckFailed(
                            "Web application requires 'Origin' header for integrity check.".into(),
                        ));
                    }
                }
                "ios" | "android" => {
                    // MOBILE INTEGRITY: Enforce presence of Attestation/Integrity Token
                    if request_headers.get("X-Integrity-Token").is_none() {
                        return Err(VaultlessError::IntegrityCheckFailed(format!(
                            "{} application requires 'X-Integrity-Token' header for integrity check.",
                            platform
                        )));
                    }
                }
                _ => {
                    // Other/Unknown platforms: No integrity check enforced
                }
            }
        }

        // 3. QUOTA AND RATE LIMIT CHECKS

        // Build Redis keys, mapping errors to VaultlessError::Internal
        let monthly_key = ApiKey::quota_cache_key(sk.id);
        let now = Utc::now();
        let period_key = MetricKey::new(sk.id, now, MetricGranularity::Minute)
            .map_err(|e| VaultlessError::Internal(format!("Failed to create metric key: {}", e)))?;

        // Fetch monthly quota and period metrics. The `?` operator uses the `From` trait
        // to convert Redis errors into VaultlessError::Internal.
        let mut conn = redis_pool.get().await?;

        let results: Vec<Option<i64>> = redis::pipe()
            .atomic()
            .get(&monthly_key)
            .hget(&period_key.as_str(), "messages_sent")
            .hget(&period_key.as_str(), "messages_received")
            .query_async(&mut *conn)
            .await?;

        let monthly_messages = results.get(0).copied().flatten().unwrap_or(0);
        let messages_sent = results.get(1).copied().flatten().unwrap_or(0);
        let messages_received = results.get(2).copied().flatten().unwrap_or(0);
        let total_requests = messages_sent + messages_received;

        // Validate quotas
        if monthly_messages >= sk.monthly_message_quota as i64 {
            return Err(VaultlessError::QuotaExceeded(
                "API key monthly quota exhausted.".into(),
            ));
        }

        // Validate rate limits
        if total_requests >= sk.rate_limit_per_minute as i64 {
            let sk_id = sk.id;
            let pool_clone = redis_pool.clone();

            tokio::spawn(async move {
                let _ =
                    increment_rate_limit_hit_pool(&*pool_clone, sk_id, &MetricsConfig::default())
                        .await;
            });

            return Err(VaultlessError::RateLimitExceeded);
        }

        Ok(())
    }

    pub async fn resolve_and_validate<'c, E>(
        exec: E,
        redis_pool: Arc<deadpool_redis::Pool>,
        key_plaintext: &str,
        granularity: &KeyGranularity,
        request_headers: &HashMap<String, String>,
    ) -> Result<Self>
    // <-- Updated signature to return Self on success, VaultlessError on failure
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Step 1: Choose resolver based on granularity
        let full_bundle_result = match granularity {
            KeyGranularity::Publishable => {
                super::Application::resolve_publishable_key_bundle(
                    exec.clone(),
                    redis_pool.clone(),
                    key_plaintext,
                )
                .await
            }
            KeyGranularity::Secret => {
                super::Application::resolve_secret_key_bundle(
                    exec.clone(),
                    redis_pool.clone(),
                    key_plaintext,
                )
                .await
            }
        };

        // Step 2: Handle resolution result
        let full_bundle = match full_bundle_result {
            Ok(bundle) => bundle,
            Err(VaultlessError::NotFound(_)) => {
                // Key not found is a specific client-facing error
                return Err(VaultlessError::NotFound(match granularity {
                    KeyGranularity::Publishable => "Publishable key not found.".into(),
                    KeyGranularity::Secret => "Secret key not found.".into(),
                }));
            }
            Err(e) => {
                // All other internal errors propagate
                return Err(e);
            }
        };

        // Step 3: Build the lean cached bundle
        let cached_bundle = CachedResolvedKeyBundle {
            application: CachedApplication::from(&full_bundle.application),
            secret_key_row: CachedApiKey::from(&full_bundle.secret_key_row),
        };

        // Step 4: Run hot validation. Any failure returns a VaultlessError directly via `?`
        cached_bundle
            .validate_hot(redis_pool.clone(), request_headers)
            .await?;

        // Step 5: Return success
        Ok(cached_bundle)
    }
}
