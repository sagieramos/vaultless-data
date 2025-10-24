use config::Config;
use deadpool_redis::Config as RedisConfig;
use lettre::{AsyncSmtpTransport, Tokio1Executor, transport::smtp::authentication::Credentials};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing_subscriber::{EnvFilter, fmt};

use crate::{job, worker};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // read env via config crate
    let settings = Config::builder()
        .add_source(config::Environment::default())
        .build()?;
    // required env vars
    let redis_url = settings.get_string("REDIS_URL")?;
    let smtp_server = settings.get_string("SMTP_SERVER")?;
    let smtp_port = settings.get_int("SMTP_PORT")? as u16;
    let smtp_user = settings.get_string("SMTP_USERNAME")?;
    let smtp_pass = settings.get_string("SMTP_PASSWORD")?;
    let from_email = settings.get_string("FROM_EMAIL")?;

    // redis pool
    let mut dp_cfg = RedisConfig::from_url(redis_url);
    let pool = dp_cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))?;

    // mailer
    let creds = Credentials::new(smtp_user, smtp_pass);
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp_server)?
        .port(smtp_port)
        .credentials(creds)
        .build();

    // start worker(s)
    let worker = Worker::new(pool.clone(), mailer, from_email.clone());

    // concurrency from env or default 4
    let concurrency: usize = settings.get_int("WORKER_CONCURRENCY").unwrap_or(4) as usize;

    // spawn worker run in a task so we can listen for shutdown signal
    let run_handle = tokio::spawn(async move {
        if let Err(e) = worker.run(concurrency).await {
            tracing::error!("Worker run ended with error: {:?}", e);
        }
    });

    // Wait for CTRL-C
    signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, exiting...");
    // when process exits, background tasks will be stopped by runtime.

    // Optionally wait for run_handle to finish
    tokio::time::sleep(Duration::from_secs(1)).await;
    Ok(())
}
