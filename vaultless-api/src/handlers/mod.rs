// vaultless-api/src/handlers/mod.rs
pub mod analytics;
pub mod api_keys;
pub mod auth;
pub mod dto;
pub mod messages;
pub mod proofs;

// Re-export message handlers
pub use messages::{get_message_metadata, receive_messages, send_message};

pub use analytics::*;
pub use api_keys::*;
pub use auth::*;
pub use messages::*;
pub use proofs::*;