use super::types::*;
use crate::error::{Result, VaultlessError};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// =============================================================================
// WEB ATTESTATION TYPES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAttestationRequest {
    pub origin: String,
    pub user_agent: String,
    pub ip_address: String,
    pub captcha_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebClientBinding {
    pub client_id: uuid::Uuid,
    pub registered_origin: String,
    pub registered_user_agent: String,
    pub registered_ip: String,
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
        if allowed.starts_with("*.") {
            let domain = &allowed[2..]; // Remove "*."
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

/// Check IP-based rate limits for registrations
pub async fn check_registration_rate_limit(
    redis_pool: &RedisPool,
    ip_address: &str,
    max_per_hour: u32,
) -> Result<()> {
    let key = format!("{}:reg:ip:{}:hour", RATE_LIMIT_PREFIX, ip_address);

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
            "Too many registrations from IP {}. Limit: {} per hour",
            ip_address, max_per_hour
        )));
    }

    Ok(())
}

/// Check IP-based rate limits for requests
pub async fn check_request_rate_limit(
    redis_pool: &RedisPool,
    ip_address: &str,
    max_per_hour: u32,
) -> Result<()> {
    let key = format!("{}:req:ip:{}:hour", RATE_LIMIT_PREFIX, ip_address);

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
            "Too many requests from IP {}. Limit: {} per hour",
            ip_address, max_per_hour
        )));
    }

    Ok(())
}

/// Check clients per IP limit
pub async fn check_clients_per_ip(
    redis_pool: &RedisPool,
    ip_address: &str,
    max_clients: u32,
) -> Result<()> {
    let key = format!("{}:clients:ip:{}", RATE_LIMIT_PREFIX, ip_address);

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
            "Too many clients from IP {}. Limit: {}",
            ip_address, max_clients
        )));
    }

    Ok(())
}

/// Register client IP address
pub async fn register_client_ip(
    redis_pool: &RedisPool,
    ip_address: &str,
    client_id: uuid::Uuid,
) -> Result<()> {
    let key = format!("{}:clients:ip:{}", RATE_LIMIT_PREFIX, ip_address);

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
    ip_address: &str,
) -> Result<()> {
    let key = format!("{}:{}", BINDING_PREFIX, client_id);

    let binding = WebClientBinding {
        client_id,
        registered_origin: origin.to_string(),
        registered_user_agent: user_agent.to_string(),
        registered_ip: ip_address.to_string(),
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
pub async fn verify_client_origin(
    redis_pool: &RedisPool,
    client_id: uuid::Uuid,
    current_origin: &str,
    max_changes: u32,
) -> Result<()> {
    let key = format!("{}:{}", BINDING_PREFIX, client_id);

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
            client_id = %client_id,
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

    Ok(())
}

// =============================================================================
// USAGE SPIKE DETECTION
// =============================================================================

const USAGE_PREFIX: &str = "browser_usage";

/// Track usage for spike detection
pub async fn track_usage(
    redis_pool: &RedisPool,
    publishable_key: &str,
) -> Result<()> {
    let key = format!("{}:pk:{}:hour", USAGE_PREFIX, publishable_key);

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
    let current_key = format!("{}:pk:{}:hour", USAGE_PREFIX, publishable_key);
    let baseline_key = format!("{}:pk:{}:baseline", USAGE_PREFIX, publishable_key);

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let current: u32 = conn
        .get(&current_key)
        .await
        .unwrap_or(0);

    let baseline: u32 = conn
        .get(&baseline_key)
        .await
        .unwrap_or(0);

    // Update baseline (rolling average)
    if current > 0 {
        let new_baseline = if baseline == 0 {
            current
        } else {
            ((baseline as f64 * 0.9) + (current as f64 * 0.1)) as u32
        };

        let _: () = conn
            .set_ex(&baseline_key, new_baseline, (baseline_hours * 3600) as u64)
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
    ip_address: &str,
    config: super::dto::BrowserIntegrityConfig,
) -> Result<()> {
    // 1. Origin validation
    if config.require_origin_header {
        let origin = extract_origin(headers)?;
        validate_origin(&origin, &config.authorized_origins)?;

        // 2. Referer validation
        if config.require_referer_header {
            let referer = extract_referer(headers);
            validate_referer(referer.as_deref(), &origin)?;
        }
    }

    // 3. Rate limiting
    check_request_rate_limit(
        redis_pool,
        ip_address,
        config.max_requests_per_ip_per_hour,
    )
    .await?;

    Ok(())
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
        assert!(validate_referer(
            Some("https://app.example.com/page"),
            "https://app.example.com"
        )
        .is_ok());

        assert!(validate_referer(
            Some("https://evil.com/page"),
            "https://app.example.com"
        )
        .is_err());

        assert!(validate_referer(None, "https://app.example.com").is_err());
    }
}