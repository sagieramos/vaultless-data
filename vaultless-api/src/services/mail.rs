use deadpool_redis::Pool;
use redis::AsyncCommands;
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailJob {
    pub id: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub attempts: u8,
    pub max_retries: u8,
    pub created_at_ts: u64,
    // optional: correlation_id, metadata, etc.
}

impl EmailJob {
    pub fn new(to: impl Into<String>, subject: impl Into<String>, body: impl Into<String>, max_retries: u8) -> Self {
        let id = Uuid::new_v4().to_string();
        let created_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        Self {
            id,
            to: to.into(),
            subject: subject.into(),
            body: body.into(),
            attempts: 0,
            max_retries,
            created_at_ts: created_at,
        }
    }
}

/// Enqueue the job into Redis list `email_queue` (LPUSH)
pub async fn enqueue_email(pool: &Pool, job: &EmailJob) -> anyhow::Result<()> {
    let mut conn = pool.get().await?;
    let payload = serde_json::to_string(job)?;
    // LPUSH into the main queue; worker uses BRPOP
    let _ : () = conn.lpush("email_queue", payload).await?;
    Ok(())
}
