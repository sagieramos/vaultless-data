use serde::Deserialize;
use std::env;

/// Header constants for auth usage
pub struct AuthHeader;

impl AuthHeader {
    pub const API_KEY: &'static str = "X-Api-Key-Id";
    pub const BEARER: &'static str = "Bearer";
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

/// Cache configuration (your code calls this `cache` elsewhere).
#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    pub url: String,
    pub max_pool_size: Option<usize>,
    pub default_ttl: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub from_email: String,
}

/// The main application configuration used throughout the API server.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub security: SecurityConfig,
    pub cache: CacheConfig,
    /// Optional mail config (worker will use these values)
    pub mail: Option<MailConfig>,
}

impl AppConfig {
    /// Load configuration from environment variables (and .env if present).
    pub fn from_env() -> anyhow::Result<Self> {
        // allow .env for local dev
        dotenvy::dotenv().ok();

        let server = ServerConfig {
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .map_err(|e| anyhow::anyhow!("PORT parse error: {}", e))?,
            log_level: env::var("RUST_LOG")
                .unwrap_or_else(|_| "info,vaultless_api=debug".to_string()),
        };

        let database = DatabaseConfig {
            url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            max_connections: env::var("DATABASE_MAX_CONNECTIONS")
                .unwrap_or_else(|_| "10".into())
                .parse()
                .map_err(|e| anyhow::anyhow!("DATABASE_MAX_CONNECTIONS parse error: {}", e))?,
        };

        let security = SecurityConfig {
            api_key_salt: env::var("API_KEY_SALT").unwrap_or_else(|_| "default-salt-change-in-production".into()),
            admin_api_key: env::var("ADMIN_API_KEY").unwrap_or_else(|_| "".into()),
        };

        let cache = CacheConfig {
            url: env::var("CACHE_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            max_pool_size: env::var("CACHE_MAX_POOL_SIZE").ok().and_then(|s| s.parse().ok()),
            default_ttl: env::var("CACHE_DEFAULT_TTL")
                .unwrap_or_else(|_| "3600".into())
                .parse()
                .map_err(|e| anyhow::anyhow!("CACHE_DEFAULT_TTL parse error: {}", e))?,
        };

        // Mail config is optional: read only if SMTP env vars present
        let mail = match env::var("SMTP_SERVER").ok() {
            Some(smtp_server) => {
                let smtp_port = env::var("SMTP_PORT")
                    .unwrap_or_else(|_| "587".to_string())
                    .parse()
                    .map_err(|e| anyhow::anyhow!("SMTP_PORT parse error: {}", e))?;
                let smtp_username = env::var("SMTP_USERNAME").unwrap_or_else(|_| "".to_string());
                let smtp_password = env::var("SMTP_PASSWORD").unwrap_or_else(|_| "".to_string());
                let from_email = env::var("FROM_EMAIL").unwrap_or_else(|_| smtp_username.clone());
                Some(MailConfig {
                    smtp_server,
                    smtp_port,
                    smtp_username,
                    smtp_password,
                    from_email,
                })
            }
            None => None,
        };

        let cfg = AppConfig {
            server,
            database,
            security,
            cache,
            mail,
        };

        // Basic validation
        if cfg.database.url.is_empty() {
            anyhow::bail!("DATABASE_URL cannot be empty");
        }
        if cfg.security.api_key_salt == "default-salt-change-in-production" {
            tracing::warn!("⚠️ Using default API_KEY_SALT - change this in production!");
        }

        Ok(cfg)
    }

    /// Helper: return bind address string
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_address() {
        let c = AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 3000,
                log_level: "info".into(),
            },
            database: DatabaseConfig {
                url: "postgres://localhost".into(),
                max_connections: 5,
            },
            security: SecurityConfig {
                api_key_salt: "s".into(),
                admin_api_key: "a".into(),
            },
            cache: CacheConfig {
                url: "redis://127.0.0.1:6379".into(),
                max_pool_size: Some(5),
                default_ttl: 3600,
            },
            mail: None,
        };

        assert_eq!(c.bind_address(), "127.0.0.1:3000");
    }
}
