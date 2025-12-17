// vaultless-api/src/config.rs
//! Application configuration module.
//!
//! Loads configuration from environment variables with explicit fields
//! for database and cache connections (host, port, password, etc.).

use serde::Deserialize;
use std::env;
use std::fmt;
use std::sync::Arc;
use vaultless_core::SessionKeyManager;

// =============================================================================
// MAIN CONFIG
// =============================================================================

/// Root application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub cache: CacheConfig,
    pub security: SecurityConfig,
    pub metrics: MetricsConfig,
}

// =============================================================================
// SERVER CONFIG
// =============================================================================

/// HTTP server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Bind host (e.g., "0.0.0.0", "127.0.0.1")
    pub host: String,
    /// Bind port (e.g., 8080)
    pub port: u16,
    /// Log level filter (e.g., "info,vaultless_api=debug")
    pub log_level: String,
}

// =============================================================================
// DATABASE CONFIG
// =============================================================================

/// PostgreSQL database configuration with explicit connection parameters
#[derive(Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Database hostname (e.g., "localhost", "db.example.com")
    pub host: String,
    /// Database port (default: 5432)
    pub port: u16,
    /// Database name
    pub name: String,
    /// Database username
    pub username: String,
    /// Database password
    pub password: String,
    /// Maximum number of connections in the pool
    pub max_connections: u32,
    /// Enable SSL mode (disable, require, prefer)
    pub ssl_mode: Option<String>,
    pub database_url: String,
}

impl DatabaseConfig {
    /// Build the PostgreSQL connection URL from explicit fields
    pub fn connection_url(&self) -> String {
        let ssl_param = self
            .ssl_mode
            .as_ref()
            .map(|m| format!("?sslmode={}", m))
            .unwrap_or_default();

        format!(
            "postgresql://{}:{}@{}:{}/{}{}",
            self.username, self.password, self.host, self.port, self.name, ssl_param
        )
    }
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("name", &self.name)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("max_connections", &self.max_connections)
            .field("ssl_mode", &self.ssl_mode)
            .field("database_url", &self.database_url)
            .finish()
    }
}

// =============================================================================
// CACHE CONFIG (Redis/Dragonfly)
// =============================================================================

/// Redis/Dragonfly cache configuration with explicit connection parameters
#[derive(Clone, Deserialize)]
pub struct CacheConfig {
    /// Cache hostname (e.g., "localhost", "redis.example.com")
    pub host: String,
    /// Cache port (default: 6379)
    pub port: u16,
    /// Cache password (optional)
    pub password: Option<String>,
    /// Database index (default: 0)
    pub database: Option<u8>,
    /// Maximum pool size
    pub max_pool_size: usize,
    /// Default TTL in seconds for cached items
    pub default_ttl: u64,
}

impl CacheConfig {
    /// Build the Redis connection URL from explicit fields
    pub fn connection_url(&self) -> String {
        let auth = self
            .password
            .as_ref()
            .map(|p| format!(":{}@", p))
            .unwrap_or_default();

        let db = self.database.unwrap_or(0);

        format!("redis://{}{}:{}/{}", auth, self.host, self.port, db)
    }
}

impl fmt::Debug for CacheConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("database", &self.database)
            .field("max_pool_size", &self.max_pool_size)
            .field("default_ttl", &self.default_ttl)
            .finish()
    }
}

// =============================================================================
// SECURITY CONFIG
// =============================================================================

/// Security and authentication configuration
#[derive(Clone)]
pub struct SecurityConfig {
    /// Salt for hashing API keys
    pub api_key_salt: String,
    /// Admin API key for privileged operations
    pub admin_api_key: Option<String>,
    /// Rate limit per minute (requests)
    pub rate_limit_per_minute: u32,
    /// Current PASETO session key (hex encoded)
    pub paseto_current_key: String,
    /// Previous PASETO session key for rotation (hex encoded, optional)
    pub paseto_previous_key: Option<String>,
    /// Session key manager (constructed from keys)
    pub session_key_manager: Arc<SessionKeyManager>,
}

impl fmt::Debug for SecurityConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecurityConfig")
            .field(
                "api_key_salt",
                &format!("<redacted: {} chars>", self.api_key_salt.len()),
            )
            .field(
                "admin_api_key",
                &self.admin_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field("rate_limit_per_minute", &self.rate_limit_per_minute)
            .field(
                "paseto_current_key",
                &format!("<redacted: {} chars>", self.paseto_current_key.len()),
            )
            .field(
                "paseto_previous_key",
                &self
                    .paseto_previous_key
                    .as_ref()
                    .map(|k| format!("<redacted: {} chars>", k.len())),
            )
            .finish()
    }
}

// =============================================================================
// METRICS CONFIG
// =============================================================================

/// Metrics collection and flushing configuration
#[derive(Debug, Clone, Deserialize)]
pub struct MetricsConfig {
    /// Maximum batch size for flushing metrics to database
    pub max_batch_size: usize,
    /// TTL for metrics in Redis (seconds)
    pub ttl_secs: u64,
    /// Interval between flush operations (seconds)
    pub flush_interval_secs: u64,
    /// Timeout for Redis operations (seconds)
    pub redis_timeout_secs: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            ttl_secs: 7200,
            flush_interval_secs: 60,
            redis_timeout_secs: 30,
        }
    }
}

// =============================================================================
// CONFIG LOADING
// =============================================================================

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        // Load PASETO keys first to validate early
        let paseto_current_key = env::var("PASETO_CLIENT_SESSION_CURRENT_KEY")
            .expect("PASETO_CLIENT_SESSION_CURRENT_KEY must be set");
        let paseto_previous_key = env::var("PASETO_CLIENT_SESSION_PREVIOUS_KEY").ok();

        let session_key_manager =
            SessionKeyManager::new(&paseto_current_key, paseto_previous_key.as_deref())?;

        let config = Config {
            // ─────────────────────────────────────────────────────────────────
            // Server
            // ─────────────────────────────────────────────────────────────────
            server: ServerConfig {
                host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("SERVER_PORT")
                    .unwrap_or_else(|_| "8080".to_string())
                    .parse()?,
                log_level: env::var("RUST_LOG")
                    .unwrap_or_else(|_| "info,vaultless_api=debug".to_string()),
            },

            // ─────────────────────────────────────────────────────────────────
            // Database (PostgreSQL)
            // ─────────────────────────────────────────────────────────────────
            database: DatabaseConfig {
                host: env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string()),
                port: env::var("DB_PORT")
                    .unwrap_or_else(|_| "5432".to_string())
                    .parse()?,
                name: env::var("DB_NAME").unwrap_or_else(|_| "vaultless_db".to_string()),
                username: env::var("DB_USERNAME").unwrap_or_else(|_| "vaultless".to_string()),
                password: env::var("DB_PASSWORD").expect("DB_PASSWORD must be set"),
                max_connections: env::var("DB_MAX_CONNECTIONS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()?,
                ssl_mode: env::var("DB_SSL_MODE").ok(),
                database_url: env::var("DATABASE_URL").unwrap_or_default(),
            },

            // ─────────────────────────────────────────────────────────────────
            // Cache (Redis/Dragonfly)
            // ─────────────────────────────────────────────────────────────────
            cache: CacheConfig {
                host: env::var("CACHE_HOST").unwrap_or_else(|_| "localhost".to_string()),
                port: env::var("CACHE_PORT")
                    .unwrap_or_else(|_| "6379".to_string())
                    .parse()?,
                password: env::var("CACHE_PASSWORD").ok(),
                database: env::var("CACHE_DATABASE").ok().and_then(|s| s.parse().ok()),
                max_pool_size: env::var("CACHE_MAX_POOL_SIZE")
                    .unwrap_or_else(|_| "20".to_string())
                    .parse()?,
                default_ttl: env::var("CACHE_DEFAULT_TTL")
                    .unwrap_or_else(|_| "3600".to_string())
                    .parse()?,
            },

            // ─────────────────────────────────────────────────────────────────
            // Security
            // ─────────────────────────────────────────────────────────────────
            security: SecurityConfig {
                api_key_salt: env::var("API_KEY_SALT").expect("API_KEY_SALT must be set"),
                admin_api_key: env::var("ADMIN_API_KEY").ok(),
                rate_limit_per_minute: env::var("RATE_LIMIT_PER_MINUTE")
                    .unwrap_or_else(|_| "60".to_string())
                    .parse()?,
                paseto_current_key: paseto_current_key.clone(),
                paseto_previous_key: paseto_previous_key.clone(),
                session_key_manager: Arc::new(session_key_manager),
            },

            // ─────────────────────────────────────────────────────────────────
            // Metrics
            // ─────────────────────────────────────────────────────────────────
            metrics: MetricsConfig {
                max_batch_size: env::var("METRICS_MAX_BATCH_SIZE")
                    .unwrap_or_else(|_| "1000".to_string())
                    .parse()?,
                ttl_secs: env::var("METRICS_TTL_SECS")
                    .unwrap_or_else(|_| "7200".to_string())
                    .parse()?,
                flush_interval_secs: env::var("METRICS_FLUSH_INTERVAL_SECS")
                    .unwrap_or_else(|_| "60".to_string())
                    .parse()?,
                redis_timeout_secs: env::var("METRICS_REDIS_TIMEOUT_SECS")
                    .unwrap_or_else(|_| "30".to_string())
                    .parse()?,
            },
        };

        // Validate configuration
        config.validate()?;

        Ok(config)
    }

    /// Validate configuration values
    fn validate(&self) -> anyhow::Result<()> {
        if self.database.host.is_empty() {
            anyhow::bail!("DB_HOST cannot be empty");
        }

        if self.database.name.is_empty() {
            anyhow::bail!("DB_NAME cannot be empty");
        }

        if self.security.api_key_salt == "change-this-random-salt-in-production" {
            tracing::warn!("⚠️  Using default API_KEY_SALT - change this in production!");
        }

        if self.security.api_key_salt.len() < 16 {
            tracing::warn!("⚠️  API_KEY_SALT is too short - use at least 16 characters");
        }

        Ok(())
    }

    /// Get server bind address as "host:port"
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_connection_url() {
        let db_config = DatabaseConfig {
            host: "localhost".to_string(),
            port: 5432,
            name: "testdb".to_string(),
            username: "testuser".to_string(),
            password: "testpass".to_string(),
            max_connections: 10,
            ssl_mode: None,
        };

        assert_eq!(
            db_config.connection_url(),
            "postgresql://testuser:testpass@localhost:5432/testdb"
        );
    }

    #[test]
    fn test_database_connection_url_with_ssl() {
        let db_config = DatabaseConfig {
            host: "db.example.com".to_string(),
            port: 5432,
            name: "proddb".to_string(),
            username: "produser".to_string(),
            password: "prodpass".to_string(),
            max_connections: 20,
            ssl_mode: Some("require".to_string()),
        };

        assert_eq!(
            db_config.connection_url(),
            "postgresql://produser:prodpass@db.example.com:5432/proddb?sslmode=require"
        );
    }

    #[test]
    fn test_cache_connection_url() {
        let cache_config = CacheConfig {
            host: "localhost".to_string(),
            port: 6379,
            password: None,
            database: None,
            max_pool_size: 10,
            default_ttl: 3600,
        };

        assert_eq!(cache_config.connection_url(), "redis://localhost:6379/0");
    }

    #[test]
    fn test_cache_connection_url_with_password() {
        let cache_config = CacheConfig {
            host: "redis.example.com".to_string(),
            port: 6380,
            password: Some("secretpass".to_string()),
            database: Some(2),
            max_pool_size: 20,
            default_ttl: 7200,
        };

        assert_eq!(
            cache_config.connection_url(),
            "redis://:secretpass@redis.example.com:6380/2"
        );
    }

    #[test]
    fn test_bind_address() {
        let key_manager = SessionKeyManager::new(
            "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899",
            None,
        )
        .unwrap();

        let config = Config {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                log_level: "info".to_string(),
            },
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                name: "testdb".to_string(),
                username: "testuser".to_string(),
                password: "testpass".to_string(),
                max_connections: 10,
                ssl_mode: None,
            },
            cache: CacheConfig {
                host: "localhost".to_string(),
                port: 6379,
                password: None,
                database: None,
                max_pool_size: 10,
                default_ttl: 3600,
            },
            security: SecurityConfig {
                api_key_salt: "test-salt-minimum16".to_string(),
                admin_api_key: Some("test-admin".to_string()),
                rate_limit_per_minute: 60,
                paseto_current_key:
                    "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899".to_string(),
                paseto_previous_key: None,
                session_key_manager: Arc::new(key_manager),
            },
            metrics: MetricsConfig::default(),
        };

        assert_eq!(config.bind_address(), "127.0.0.1:3000");
    }
}
