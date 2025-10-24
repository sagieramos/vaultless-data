use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailJob {
    pub id: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub attempts: u8,
    pub max_retries: u8,
    pub created_at_ts: u64,
}
