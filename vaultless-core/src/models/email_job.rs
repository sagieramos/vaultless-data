use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailJob {
    pub id: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub attempts: u8,
    pub created_at: i64,
}

impl EmailJob {
    pub fn new(to: &str, subject: &str, body: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            to: to.to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            attempts: 0,
            created_at: Utc::now().timestamp(),
        }
    }
}
