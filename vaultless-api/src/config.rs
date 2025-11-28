// vaultless-api/src/config.rs
use std::env;
use std::fmt;
use std::sync::Arc;
use vaultless_core::SessionKeyManager;
// Note: We only need Deserialize for nested configs if you plan to use something like 'config' crate later.
// For manual env loading, we don't strictly need it, but I'll leave it on leaf structs just in case.
use serde::Deserialize;

pub struct AuthHeader;

impl AuthHeader {
    pub const API_KEY: &'static str = "X-Api-Key-Id ";
    pub const BEARER: &'static str = "Bearer ";
}

#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub security: SecurityConfig,
    pub cache: CacheConfig,
    // Metrics configuration (optional with defaults)
    pub metrics_max_batch_size: Option<usize>,
    pub metrics_ttl_secs: Option<u64>,
    pub metrics_flush_interval_secs: Option<u64>,
    pub metrics_redis_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub log_level: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Clone)]
pub struct SecurityConfig {
    pub api_key_salt: String,
    pub admin_api_key: String,
    /// Raw hex keys loaded from env
    pub paseto_client_session_current_key: String,
    pub paseto_client_session_previous_key: Option<String>,

    /// Not deserialized — injected after config load
    pub paseto_client_session_key_manager: Arc<SessionKeyManager>,
}

impl fmt::Debug for SecurityConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecurityConfig")
            .field(
                "api_key_salt",
                &format!("<redacted: {} chars>", self.api_key_salt.len()),
            )
            .field("admin_api_key", &"<redacted>")
            .field(
                "paseto_client_session_current_key",
                &format!(
                    "<redacted: {} chars>",
                    self.paseto_client_session_current_key.len()
                ),
            )
            .field(
                "paseto_client_session_previous_key",
                &self
                    .paseto_client_session_previous_key
                    .as_ref()
                    .map(|k| format!("<redacted: {} chars>", k.len())),
            )
            .field(
                "paseto_client_session_key_manager",
                &self.paseto_client_session_key_manager,
            )
            .finish()
    }
}

/// Dragonfly/Redis cache configuration
#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    /// Redis/Dragonfly URL
    pub url: String,
    /// Max pool size
    pub max_pool_size: Option<usize>,
    /// Default TTL in seconds
    pub default_ttl: u64,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok(); // Load .env file if it exists

        // Raw keys from environment
        let current_key_hex = env::var("PASETO_CLIENT_SESSION_CURRENT_KEY")
            .expect("PASETO_CLIENT_SESSION_CURRENT_KEY must be set");

        let previous_key_hex = env::var("PASETO_CLIENT_SESSION_PREVIOUS_KEY").ok();

        // Create the manager first to catch errors early
        let key_manager = SessionKeyManager::new(&current_key_hex, previous_key_hex.as_deref())?;

        let config = Config {
            server: ServerConfig {
                host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("PORT")
                    .unwrap_or_else(|_| "8080".to_string())
                    .parse()?,
                log_level: env::var("RUST_LOG")
                    .unwrap_or_else(|_| "info,vaultless_api=debug".to_string()),
            },
            database: DatabaseConfig {
                url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
                max_connections: env::var("DATABASE_MAX_CONNECTIONS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()?,
            },
            security: SecurityConfig {
                api_key_salt: env::var("API_KEY_SALT").expect("API_KEY_SALT must be set"),
                admin_api_key: env::var("ADMIN_API_KEY").unwrap_or_else(|_| "".to_string()),
                paseto_client_session_current_key: current_key_hex.clone(),
                paseto_client_session_previous_key: previous_key_hex.clone(),
                // 3. Wrap in Arc here
                paseto_client_session_key_manager: Arc::new(key_manager),
            },
            cache: CacheConfig {
                url: env::var("CACHE_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
                max_pool_size: env::var("CACHE_MAX_POOL_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok()),
                default_ttl: env::var("CACHE_DEFAULT_TTL")
                    .unwrap_or_else(|_| "3600".to_string())
                    .parse()?,
            },
            // Metrics configuration (optional)
            metrics_max_batch_size: env::var("METRICS_MAX_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok()),
            metrics_ttl_secs: env::var("METRICS_TTL_SECS")
                .ok()
                .and_then(|s| s.parse().ok()),
            metrics_flush_interval_secs: env::var("METRICS_FLUSH_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok()),
            metrics_redis_timeout_secs: env::var("METRICS_REDIS_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok()),
        };

        // Validate critical config
        if config.database.url.is_empty() {
            anyhow::bail!("DATABASE_URL cannot be empty");
        }

        if config.security.api_key_salt == "default-salt-change-in-production" {
            tracing::warn!("⚠️  Using default API_KEY_SALT - change this in production!");
        }

        Ok(config)
    }

    /// Get server bind address
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_address() {
        // 4. Update the test to wrap in Arc
        let key_manager = SessionKeyManager::new("aabbccddeeff00112233445566778899", None).unwrap();

        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                log_level: "info".to_string(),
            },
            database: DatabaseConfig {
                url: "postgres://localhost".to_string(),
                max_connections: 10,
            },
            security: SecurityConfig {
                api_key_salt: "test-salt".to_string(),
                admin_api_key: "test-admin".to_string(),
                paseto_client_session_current_key: "aabbccddeeff00112233445566778899".to_string(),
                paseto_client_session_previous_key: None,
                paseto_client_session_key_manager: Arc::new(key_manager), // Corrected here
            },
            cache: CacheConfig {
                url: "redis://127.0.0.1:6379".to_string(),
                max_pool_size: Some(5),
                default_ttl: 3600,
            },
            metrics_max_batch_size: Some(1000),
            metrics_ttl_secs: Some(2592000),
            metrics_flush_interval_secs: Some(60),
            metrics_redis_timeout_secs: Some(5),
        };

        assert_eq!(config.bind_address(), "127.0.0.1:3000");
    }
}
