use crate::{
    cache_key,
    error::{Result, VaultlessError},
    models::app_model::integrity::captcha::{CaptchaProvider, verify_captcha},
};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;

// =============================================================================
// BROWSER INTEGRITY TYPES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserIntegrityRequest {
    pub origin: String,
    pub user_agent: String,
    pub ip_address: IpAddr,
    pub captcha_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebClientBinding {
    pub client_id: uuid::Uuid,
    pub registered_origin: String,
    pub registered_user_agent: String,
    pub registered_ip: IpAddr,
    pub origin_changes: u32,
    pub last_origin_change: Option<chrono::DateTime<Utc>>,
}

// =============================================================================
// ORIGIN VALIDATION
// =============================================================================

/// Validate origin header against allowed origins
pub fn validate_origin(origin: &str, allowed_origins: &[String]) -> Result<()> {
    if allowed_origins.is_empty() {
        // No restrictions configured - allow all (dev mode)
        return Ok(());
    }

    // Exact match required
    if allowed_origins.iter().any(|allowed| allowed == origin) {
        return Ok(());
    }

    // Check wildcard subdomain patterns (*.example.com)
    for allowed in allowed_origins {
        if let Some(domain) = allowed.strip_prefix("*.") {
            // Remove "*."
            if origin.ends_with(domain) {
                return Ok(());
            }
        }
    }

    Err(VaultlessError::IntegrityCheckFailed(format!(
        "Origin '{}' not in authorized list",
        origin
    )))
}

/// Validate referer header consistency with origin
pub fn validate_referer(referer: Option<&str>, origin: &str) -> Result<()> {
    let Some(referer) = referer else {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Missing Referer header".into(),
        ));
    };

    // Referer should start with origin
    if !referer.starts_with(origin) {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Referer does not match Origin".into(),
        ));
    }

    Ok(())
}

/// Extract origin from request headers
pub fn extract_origin(headers: &HashMap<String, String>) -> Result<String> {
    headers
        .get("origin")
        .or_else(|| headers.get("Origin"))
        .cloned()
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing Origin header".into()))
}

/// Extract referer from request headers
pub fn extract_referer(headers: &HashMap<String, String>) -> Option<String> {
    headers
        .get("referer")
        .or_else(|| headers.get("Referer"))
        .cloned()
}

// =============================================================================
// RATE LIMITING
// =============================================================================

const RATE_LIMIT_PREFIX: &str = "browser_rate_limit";

/// Check rate limits for registrations (using identity key)
pub async fn check_registration_rate_limit(
    redis_pool: &RedisPool,
    identity_key: &str,
    max_per_hour: u32,
) -> Result<()> {
    let key = format!("{}:reg:{identity_key}:hour", RATE_LIMIT_PREFIX);

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let count: u32 = conn
        .incr(&key, 1)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis INCR failed: {}", e)))?;

    if count == 1 {
        // Set expiry on first increment
        let _: () = conn
            .expire(&key, 3600)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;
    }

    if count > max_per_hour {
        return Err(VaultlessError::RateLimitExceeded(format!(
            "Too many registrations from identity {}. Limit: {} per hour",
            identity_key, max_per_hour
        )));
    }

    Ok(())
}

/// Check rate limits for requests (using identity key)
pub async fn check_request_rate_limit(
    redis_pool: &RedisPool,
    identity_key: &str,
    max_per_hour: u32,
) -> Result<()> {
    let key = cache_key!(RATE_LIMIT_PREFIX, "req", identity_key, "hour");

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let count: u32 = conn
        .incr(&key, 1)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis INCR failed: {}", e)))?;

    if count == 1 {
        let _: () = conn
            .expire(&key, 3600)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;
    }

    if count > max_per_hour {
        return Err(VaultlessError::RateLimitExceeded(format!(
            "Too many requests from identity {}. Limit: {} per hour",
            identity_key, max_per_hour
        )));
    }

    Ok(())
}

/// Check clients per identity limit
pub async fn check_clients_per_identity(
    redis_pool: &RedisPool,
    identity_key: &str,
    max_clients: u32,
) -> Result<()> {
    let key = format!("{}:clients:{}", RATE_LIMIT_PREFIX, identity_key);

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let count: u32 = conn
        .scard(&key)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis SCARD failed: {}", e)))?;

    if count >= max_clients {
        return Err(VaultlessError::RateLimitExceeded(format!(
            "Too many clients from identity {}. Limit: {}",
            identity_key, max_clients
        )));
    }

    Ok(())
}

/// Register client identity
pub async fn register_client_identity(
    redis_pool: &RedisPool,
    identity_key: &str,
    client_id: uuid::Uuid,
) -> Result<()> {
    let key = cache_key!(RATE_LIMIT_PREFIX, "clients", identity_key);

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let _: () = conn
        .sadd(&key, client_id.to_string())
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis SADD failed: {}", e)))?;

    // Expire after 24 hours
    let _: () = conn
        .expire(&key, 86400)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;

    Ok(())
}

// =============================================================================
// CLIENT-ORIGIN BINDING
// =============================================================================

const BINDING_PREFIX: &str = "browser_client_binding";

/// Store client-origin binding
pub async fn bind_client_to_origin(
    redis_pool: &RedisPool,
    client_id: uuid::Uuid,
    origin: &str,
    user_agent: &str,
    ip_address: &IpAddr,
) -> Result<()> {
    let key = cache_key!(BINDING_PREFIX, client_id);

    let binding = WebClientBinding {
        client_id,
        registered_origin: origin.to_string(),
        registered_user_agent: user_agent.to_string(),
        registered_ip: *ip_address,
        origin_changes: 0,
        last_origin_change: None,
    };

    let value = serde_json::to_string(&binding)
        .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let _: () = conn
        .set_ex(&key, value, 86400 * 30) // 30 days
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis SET failed: {}", e)))?;

    Ok(())
}

/// Verify client origin consistency
///
/// This function enforces binding rules only when client_id is present.
/// For guest users (IP-only), the origin verification is skipped.
pub async fn verify_client_origin(
    redis_pool: &RedisPool,
    client_id: Option<uuid::Uuid>,
    current_origin: &str,
    max_changes: u32,
) -> Result<()> {
    // Only enforce binding for registered clients, skip for guests
    if let Some(id) = client_id {
        let key = cache_key!(BINDING_PREFIX, id);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

        let binding_json: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis GET failed: {}", e)))?;

        let Some(binding_json) = binding_json else {
            // No binding found - first time or expired
            return Ok(());
        };

        let mut binding: WebClientBinding = serde_json::from_str(&binding_json)
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

        // Check if origin matches
        if binding.registered_origin != current_origin {
            binding.origin_changes += 1;
            binding.last_origin_change = Some(Utc::now());

            tracing::warn!(
                client_id = %id,
                original_origin = %binding.registered_origin,
                current_origin = %current_origin,
                changes = binding.origin_changes,
                "Client origin changed"
            );

            if binding.origin_changes > max_changes {
                return Err(VaultlessError::IntegrityCheckFailed(format!(
                    "Client exceeded max origin changes ({}). Possible stolen key.",
                    max_changes
                )));
            }

            // Update binding
            let value = serde_json::to_string(&binding)
                .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

            let _: () = conn
                .set_ex(&key, value, 86400 * 30)
                .await
                .map_err(|e| VaultlessError::Internal(format!("Redis SET failed: {}", e)))?;
        }
    }
    // For guest users (client_id is None), we skip binding checks

    Ok(())
}

// =============================================================================
// USAGE SPIKE DETECTION
// =============================================================================

const USAGE_PREFIX: &str = "browser_usage";

/// Track usage for spike detection
pub async fn track_usage(redis_pool: &RedisPool, publishable_key: &str) -> Result<()> {
    let key = cache_key!(USAGE_PREFIX, "pk", publishable_key, "hour");

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let count: u32 = conn
        .incr(&key, 1)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis INCR failed: {}", e)))?;

    if count == 1 {
        let _: () = conn
            .expire(&key, 3600)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;
    }

    Ok(())
}

/// Check for usage spikes
pub async fn check_usage_spike(
    redis_pool: &RedisPool,
    publishable_key: &str,
    threshold: f64,
    baseline_hours: u64,
) -> Result<bool> {
    let current_key = cache_key!(USAGE_PREFIX, "pk", publishable_key, "hour");
    let baseline_key = cache_key!(USAGE_PREFIX, "pk", publishable_key, "baseline");

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let current: u32 = conn.get(&current_key).await.unwrap_or(0);

    let baseline: u32 = conn.get(&baseline_key).await.unwrap_or(0);

    // Update baseline (rolling average)
    if current > 0 {
        let new_baseline = if baseline == 0 {
            current
        } else {
            ((baseline as f64 * 0.9) + (current as f64 * 0.1)) as u32
        };

        let _: () = conn
            .set_ex(&baseline_key, new_baseline, baseline_hours * 3600)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis SET failed: {}", e)))?;
    }

    // Check for spike
    if baseline > 0 && current as f64 > baseline as f64 * threshold {
        tracing::warn!(
            publishable_key = %publishable_key,
            current_usage = current,
            baseline = baseline,
            threshold = threshold,
            "Usage spike detected"
        );
        return Ok(true);
    }

    Ok(false)
}

// =============================================================================
// COMPREHENSIVE WEB VALIDATION
// =============================================================================

/// Validate browser request with all checks
pub async fn validate_browser_request(
    redis_pool: &RedisPool,
    headers: &HashMap<String, String>,
    identity_key: &str,
    config: super::dto::BrowserIntegrityConfig,
) -> Result<()> {
    // 1. Origin validation
    if config.require_origin_header.unwrap_or(true) {
        let origin = extract_origin(headers)?;
        validate_origin(&origin, &config.authorized_origins)?;

        // 2. Referer validation
        if config.require_referer_header.unwrap_or(true) {
            let referer = extract_referer(headers);
            validate_referer(referer.as_deref(), &origin)?;
        }
    }

    // 3. Rate limiting
    check_request_rate_limit(
        redis_pool,
        identity_key,
        config.max_requests_per_ip_per_hour.unwrap_or(1000),
    )
    .await?;

    Ok(())
}

/// Validate browser integrity and produce a standardized integrity result
pub async fn validate_browser_integrity(
    redis_pool: &RedisPool,
    browser_data: &super::types::BrowserData,
    identity_key: &str,
    client_id: Option<uuid::Uuid>,
    ip_address: std::net::IpAddr,
    config: &super::dto::BrowserIntegrityConfig,
) -> Result<super::types::AttestationResult> {
    use chrono::Utc;
    use serde_json::Value as jsonValue;

    // 1. Origin validation - origin, referer, user_agent should be populated by middleware/handler
    if config.require_origin_header.unwrap_or(true) {
        // Check if required fields from headers are present
        if let Some(ref origin) = browser_data.origin {
            validate_origin(origin, &config.authorized_origins)?;

            // 2. Referer validation
            if config.require_referer_header.unwrap_or(true) {
                validate_referer(browser_data.referer.as_deref(), origin)?;
            }
        } else {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Missing Origin header".into(),
            ));
        }
    }

    // 3. CAPTCHA validation (if required)
    if config.require_captcha_on_registration.unwrap_or(false) {
        if let Some(ref captcha_token) = browser_data.captcha_token {
            // Determine CAPTCHA provider and verify token
            let captcha_provider = config.captcha_provider
                .as_deref()
                .unwrap_or("turnstile");

            let captcha_secret = config
                .captcha_secret_key
                .as_deref()
                .ok_or_else(|| {
                    VaultlessError::IntegrityCheckFailed(
                        "CAPTCHA secret key not configured".into()
                    )
                })?;

            let verified = verify_captcha(
                match captcha_provider {
                    "turnstile" => CaptchaProvider::Turnstile,
                    "hcaptcha" => CaptchaProvider::HCaptcha,
                    "recaptcha" => CaptchaProvider::ReCaptcha,
                    _ => {
                        return Err(VaultlessError::IntegrityCheckFailed(
                            "Invalid CAPTCHA provider".into(),
                        ));
                    }
                },
                captcha_token,
                captcha_secret,
                config.captcha_site_key.as_deref(),
                Some(ip_address.to_string().as_str()), // IP address for additional validation
            )
            .await?;

            if !verified {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "CAPTCHA verification failed".into(),
                ));
            }
        } else {
            return Err(VaultlessError::IntegrityCheckFailed(
                "CAPTCHA token required but not provided".into(),
            ));
        }
    }

    // 4. Client-origin binding (only for registered clients)
    if let Some(client_id_val) = client_id {
        if let Some(ref origin) = browser_data.origin {
            verify_client_origin(
                redis_pool,
                Some(client_id_val),
                origin, // current origin
                config.max_origin_changes_per_client.unwrap_or(3),
            ).await?;

            // 5. Bind client to origin/user-agent if required
            if config.bind_client_to_origin.unwrap_or(false) {
                if let Some(user_agent) = &browser_data.user_agent {
                    bind_client_to_origin(
                        redis_pool,
                        client_id_val,
                        origin,
                        user_agent,
                        &ip_address,
                    ).await?;
                } else {
                    return Err(VaultlessError::IntegrityCheckFailed(
                        "Missing User-Agent header for client binding".into(),
                    ));
                }
            }
        } else {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Missing Origin header for client binding".into(),
            ));
        }
    }

    // 6. Rate limiting checks
    check_request_rate_limit(
        redis_pool,
        identity_key,
        config.max_requests_per_ip_per_hour.unwrap_or(1000),
    )
    .await?;

    // 7. Check max registrations per IP if applicable
    if let Some(max_registrations) = config.max_registrations_per_ip_per_hour {
        check_registration_rate_limit(
            redis_pool,
            identity_key,
            max_registrations,
        ).await?;
    }

    // 8. Check max clients per IP
    if let Some(max_clients) = config.max_clients_per_ip {
        check_clients_per_identity(
            redis_pool,
            identity_key,
            max_clients,
        ).await?;
    }

    // 9. Usage spike detection
    if config.alert_on_usage_spike.unwrap_or(false) {
        let spike_detected = check_usage_spike(
            redis_pool,
            identity_key,
            config.usage_spike_threshold.unwrap_or(3.0),
            config.usage_baseline_hours.unwrap_or(24),
        ).await?;

        if spike_detected {
            tracing::warn!(
                identity_key = %identity_key,
                "Usage spike detected for IP: {}", ip_address
            );
        }
    }

    // 10. Track usage for potential future analysis
    track_usage(redis_pool, identity_key).await?;

    // 11. Create attestation result with trust score
    let trust_score = config.calculate_trust_score();

    Ok(super::types::AttestationResult {
        is_valid: true,
        device_trusted: true, // Browsers don't have hardware attestation
        trust_score_percent: trust_score,
        verdict: Some("Browser integrity validation passed".to_string()),
        error: None,
        warnings: None,
        verified_at: Utc::now(),
        extra: jsonValue::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_origin_exact_match() {
        let allowed = vec!["https://app.example.com".to_string()];
        assert!(validate_origin("https://app.example.com", &allowed).is_ok());
        assert!(validate_origin("https://evil.com", &allowed).is_err());
    }

    #[test]
    fn test_validate_origin_wildcard() {
        let allowed = vec!["*.example.com".to_string()];
        assert!(validate_origin("https://app.example.com", &allowed).is_ok());
        assert!(validate_origin("https://staging.example.com", &allowed).is_ok());
        assert!(validate_origin("https://example.com", &allowed).is_err());
        assert!(validate_origin("https://evil.com", &allowed).is_err());
    }

    #[test]
    fn test_validate_origin_empty_allows_all() {
        let allowed = vec![];
        assert!(validate_origin("https://anything.com", &allowed).is_ok());
    }

    #[test]
    fn test_validate_referer() {
        assert!(
            validate_referer(
                Some("https://app.example.com/page"),
                "https://app.example.com"
            )
            .is_ok()
        );

        assert!(
            validate_referer(Some("https://evil.com/page"), "https://app.example.com").is_err()
        );

        assert!(validate_referer(None, "https://app.example.com").is_err());
    }
}
