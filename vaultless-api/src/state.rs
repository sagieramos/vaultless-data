// api/src/state.rs
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;
use tera::Tera;

use crate::config::AppConfig;
use crate::services::cache::CacheService;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub redis_pool: RedisPool,
    pub config: Arc<AppConfig>,
    pub tera: Arc<Tera>,
}

impl AppState {
    /// Construct an AppState from already-created pools and config.
    /// This keeps AppState construction synchronous and predictable for your main.rs.
    pub fn new(db: PgPool, redis_pool: RedisPool, config: AppConfig) -> Self {
        // Load templates (gracefully fall back to empty Tera)
        let tera = match Tera::new("templates/emails/**/*") {
            Ok(t) => {
                tracing::info!("✅ Email templates loaded successfully");
                t
            }
            Err(e) => {
                tracing::warn!("⚠️ Failed to load email templates: {}. Using empty Tera instance.", e);
                Tera::default()
            }
        };

        Self {
            db,
            redis_pool,
            config: Arc::new(config),
            tera: Arc::new(tera),
        }
    }

    /// Convenience: create a CacheService instance using configured TTL
    pub fn cache_service(&self) -> CacheService {
        CacheService::new(self.redis_pool.clone(), self.config.cache.default_ttl)
    }

    /// Whether mail is configured (useful for disabling mail endpoints in API if worker isn't configured)
    pub fn has_mail_config(&self) -> bool {
        self.config.mail.is_some()
    }

    /// Return a reference to the MailConfig if present
    pub fn mail_config(&self) -> Option<&crate::config::MailConfig> {
        self.config.mail.as_ref()
    }

    /// Helper getters for ergonomics
    pub fn db_pool(&self) -> &PgPool {
        &self.db
    }

    pub fn redis(&self) -> &RedisPool {
        &self.redis_pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, CacheConfig, DatabaseConfig, SecurityConfig, ServerConfig};

    fn create_test_config() -> AppConfig {
        AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                log_level: "info".to_string(),
            },
            database: DatabaseConfig {
                url: "postgres://localhost/test".to_string(),
                max_connections: 5,
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
            mail: None,
        }
    }

    #[test]
    fn test_has_mail_config_none() {
        let config = create_test_config();
        assert!(config.mail.is_none());
    }

    #[test]
    fn test_cache_ttl_from_config() {
        let config = create_test_config();
        assert_eq!(config.cache.default_ttl, 3600);
    }
}
