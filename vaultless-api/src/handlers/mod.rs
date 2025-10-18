pub mod admin;
pub mod analytics;
pub mod api_keys;
pub mod auth;
pub mod dto;
pub mod messages;
pub mod rate_limit_monitoring;
pub mod proofs;

// Re-export admin handlers
pub use admin::{create_api_key, get_rate_limit_status, list_api_keys, reset_rate_limit};

// Re-export analytics handlers
pub use analytics::{get_daily_usage, get_dashboard, get_weekly_usage};

// Re-export message handlers
pub use messages::{get_message_metadata, receive_messages, send_message};

// Re-export rate limit monitoring handlers
pub use rate_limit_monitoring::{get_my_rate_limit_status, get_rate_limit_history};

pub use auth::*;
pub use messages::*;
pub use proofs::*;
pub use api_keys::*;
