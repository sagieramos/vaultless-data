// vaultless-api/src/config.rs
use serde::Deserialize;
use std::env;

pub struct AuthHeader;

impl AuthHeader {
    pub const API_KEY: &'static str = "X-Api-Key-Id ";
    pub const BEARER: &'static str = "Bearer ";
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub api_key_salt: String,
    pub admin_api_key: String,
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
