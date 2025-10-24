use tracing_subscriber::{fmt, EnvFilter};
use config::Config;
use deadpool_redis::Config as RedisConfig;
use lettre::{transport::smtp::authentication::Credentials, AsyncSmtpTransport, Tokio1Executor};
use redis::AsyncCommands;

mod config;
mod job;
mod worker;

use config::WorkerConfig;
use worker::Worker;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cfg = WorkerConfig::from_env()?;
    // build redis pool
    let mut dp_cfg = RedisConfig::from_url(cfg.redis_url.clone());
    let pool = dp_cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))?;

    // ensure consumer group exists (XGROUP CREATE ... MKSTREAM)
    {
        let mut conn = pool.get().await?;
        let stream_key = cfg.stream_key.clone().unwrap_or_else(|| "email_stream".to_string());
        let group = cfg.consumer_group.clone().unwrap_or_else(|| "email_consumers".to_string());

        let res: Result<(), redis::RedisError> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&stream_key)
            .arg(&group)
            .arg("$")
            .arg("MKSTREAM")
            .query_async(&mut *conn)
            .await;
        match res {
            Ok(_) => tracing::info!("Consumer group created"),
            Err(e) => {
                let s = e.to_string();
                if s.contains("BUSYGROUP") {
                    tracing::info!("Consumer group already exists");
                } else {
                    tracing::error!("Failed to create consumer group: {:?}", e);
                    return Err(anyhow::anyhow!(e));
                }
            }
        }
    }

    // Make mailer
    let creds = Credentials::new(cfg.smtp_username.clone(), cfg.smtp_password.clone());
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.smtp_server)?
        .port(cfg.smtp_port)
        .credentials(creds)
        .build();

    let worker = Worker::new(pool.clone(), mailer, cfg.clone());

    // Run worker (this will spawn internal tasks)
    worker.run().await?;

    Ok(())
}
