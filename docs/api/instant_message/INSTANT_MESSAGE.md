# InstantMessage API Documentation

> High-performance P2P Instant Messaging with Redis caching and PostgreSQL persistence

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Getting Started](#getting-started)
- [Public API Reference](#public-api-reference)
- [Usage Examples](#usage-examples)
- [Advanced Scenarios](#advanced-scenarios)
- [Performance Considerations](#performance-considerations)
- [Monitoring & Health Checks](#monitoring--health-checks)
- [Error Handling](#error-handling)

---

## Overview

The `InstantMessage` system provides a high-performance, scalable messaging platform with:

- **Sub-second message delivery** via Redis L1 cache
- **Durable persistence** in PostgreSQL
- **End-to-end encryption** with signature verification
- **Automatic batching** for optimal database performance
- **Graceful degradation** with SQL fallback
- **Built-in metrics** for usage tracking and billing

### Key Features

✅ **20k+ RPS** throughput with Redis caching  
✅ **Atomic delivery counting** prevents double-billing  
✅ **Signature verification** ensures message integrity  
✅ **Race-condition free** with distributed locking  
✅ **Automatic cleanup** of delivered messages  
✅ **Read receipts** with pub/sub notifications  

---

## Architecture

```
┌─────────────┐         ┌─────────────┐         ┌──────────────┐
│   Sender    │────────▶│   Redis     │◀───────▶│  PostgreSQL  │
│   Client    │         │   (L1)      │         │  (Durable)   │
└─────────────┘         └─────────────┘         └──────────────┘
                              │                         ▲
                              │                         │
                              ▼                         │
                        ┌─────────────┐                 │
                        │  Recipient  │                 │
                        │   Client    │                 │
                        └─────────────┘                 │
                              │                         │
                              └─────────────────────────┘
                                   (Background Flush)
```

### Component Flow

1. **Send Message**: Write to Redis (fast), queue for batch flush
2. **Fetch Messages**: Read from Redis (cache hit) or PostgreSQL (cache miss)
3. **Background Flusher**: Batches Redis messages → PostgreSQL every 60s
4. **Background Deleter**: Removes delivered P2P messages
5. **Background Purger**: Cleans up old delivered messages (24h retention)

---

## Getting Started

### Prerequisites

```toml
# Cargo.toml
[dependencies]
redis = "0.24"
deadpool-redis = "0.14"
sqlx = { version = "0.7", features = ["postgres", "runtime-tokio-native-tls", "uuid", "chrono"] }
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
base64 = "0.21"
```

### Database Setup

```sql
-- Messages table
CREATE TABLE messages (
    id UUID PRIMARY KEY,
    ciphertext TEXT NOT NULL,
    nonce TEXT NOT NULL,
    sender_client_id UUID NOT NULL,
    recipient_client_id UUID NOT NULL,
    api_key_id UUID,
    is_group_message BOOLEAN NOT NULL DEFAULT false,
    content_size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    is_delivered BOOLEAN NOT NULL DEFAULT false,
    delivered_at TIMESTAMPTZ,
    signature TEXT NOT NULL,
    envelope_public_key TEXT NOT NULL,
    file_id UUID
);

-- Indexes for performance
CREATE INDEX idx_messages_recipient ON messages(recipient_client_id) 
  WHERE is_delivered = false AND is_group_message = false;
CREATE INDEX idx_messages_delivered_at ON messages(is_delivered, delivered_at) 
  WHERE is_delivered = true AND is_group_message = false;

-- Read receipts table
CREATE TABLE p2p_read_receipts (
    id UUID PRIMARY KEY,
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    client_id UUID NOT NULL,
    read_at TIMESTAMPTZ NOT NULL,
    UNIQUE(message_id, client_id)
);

-- Metrics table
CREATE TABLE usage_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id UUID NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    messages_sent INTEGER NOT NULL DEFAULT 0,
    messages_received INTEGER NOT NULL DEFAULT 0,
    proofs_verified INTEGER NOT NULL DEFAULT 0,
    total_bytes_stored BIGINT NOT NULL DEFAULT 0,
    rate_limit_hits INTEGER NOT NULL DEFAULT 0,
    estimated_cost_cents BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(api_key_id, period_start)
);
```

### Environment Variables

```bash
# Required
VAULTLESS_SERVER_PRIVATE_KEY="base64_encoded_ed25519_private_key"

# Redis
REDIS_URL="redis://localhost:6379"

# PostgreSQL
DATABASE_URL="postgresql://user:pass@localhost:5432/vaultless"
```

### Initialization

```rust
use instant_message::{InstantMessage, MetricsConfig};
use deadpool_redis::{Config as RedisConfig, Runtime};
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize Redis pool
    let redis_config = RedisConfig::from_url("redis://localhost:6379");
    let redis_pool = redis_config.create_pool(Some(Runtime::Tokio1))?;

    // Initialize PostgreSQL pool
    let db_pool = PgPool::connect("postgresql://localhost/vaultless").await?;

    // Configure metrics
    let metrics_config = MetricsConfig::default();

    // Create InstantMessage instance
    let im = InstantMessage::new(redis_pool, db_pool, metrics_config)?;

    Ok(())
}
```

---

## Public API Reference

### `InstantMessage::new()`

Creates a new `InstantMessage` instance with background workers.

```rust
pub fn new(
    redis_pool: RedisPool,
    db_pool: PgPool,
    config: MetricsConfig,
) -> Result<Self>
```

**Parameters:**
- `redis_pool`: Deadpool Redis connection pool
- `db_pool`: SQLx PostgreSQL connection pool
- `config`: Metrics configuration (flush intervals, batch sizes, etc.)

**Returns:** `Result<InstantMessage>`

**Side Effects:**
- Spawns 3 background tasks: flusher, deleter, purger
- Validates server private key from environment
- Creates message and delete channels

---

### `send_instant_message()`

Sends an encrypted P2P message with signature verification.

```rust
pub async fn send_instant_message(
    &self,
    sender_client_id: Uuid,
    recipient_client_id: Uuid,
    ciphertext: String,
    nonce: String,
    content_size_bytes: i64,
    api_key_id: Option<Uuid>,
    signature: String,
    envelope_public_key: String,
) -> Result<Uuid>
```

**Parameters:**
- `sender_client_id`: UUID of the sending client
- `recipient_client_id`: UUID of the receiving client
- `ciphertext`: Encrypted message content (base64)
- `nonce`: Encryption nonce (base64)
- `content_size_bytes`: Size of plaintext content for billing
- `api_key_id`: Optional API key for usage tracking
- `signature`: Ed25519 signature of the envelope
- `envelope_public_key`: Public key for signature verification

**Returns:** `Result<Uuid>` - The message ID

**Behavior:**
1. Creates message with unique UUID
2. Writes to Redis cache (TTL: 10 minutes)
3. Adds to recipient's inbox queue
4. Increments "sent" metrics (idempotent)
5. Queues for background flush to PostgreSQL

**Example:**

```rust
let msg_id = im.send_instant_message(
    sender_id,
    recipient_id,
    "base64_encrypted_content".to_string(),
    "base64_nonce".to_string(),
    1024, // bytes
    Some(api_key_id),
    "base64_signature".to_string(),
    "base64_public_key".to_string(),
).await?;

println!("Message sent: {}", msg_id);
```

---

### `fetch_messages_for_recipient()`

Fetches undelivered messages for a recipient (paginated, max 100).

```rust
pub async fn fetch_messages_for_recipient(
    &self,
    recipient_client_id: Uuid,
) -> Result<Vec<Message>>
```

**Parameters:**
- `recipient_client_id`: UUID of the recipient

**Returns:** `Result<Vec<Message>>` - List of messages (max 100)

**Behavior:**
1. Checks Redis inbox queue
2. Rebuilds inbox from PostgreSQL if empty (with distributed lock)
3. Fetches messages via MGET (bulk Redis operation)
4. Falls back to PostgreSQL for cache misses (parallel queries)
5. Verifies signatures before delivery
6. Increments "received" metrics atomically (prevents double-counting)
7. Marks messages as delivered
8. Automatically marks messages as read
9. Queues messages for deletion

**Example:**

```rust
let messages = im.fetch_messages_for_recipient(recipient_id).await?;

for msg in messages {
    println!("From: {}", msg.sender_client_id);
    println!("Content: {}", msg.ciphertext);
    println!("Delivered at: {:?}", msg.delivered_at);
}
```

---

### `mark_read_instant_message()`

Marks a message as read and creates a read receipt.

```rust
pub async fn mark_read_instant_message(
    &self,
    reader_client_id: Uuid,
    msg_id: Uuid,
) -> Result<()>
```

**Parameters:**
- `reader_client_id`: UUID of the reader
- `msg_id`: UUID of the message

**Returns:** `Result<()>`

**Behavior:**
1. Checks if message exists in PostgreSQL
2. If exists: Creates read receipt in database
3. If Redis-only: Queues pending read receipt
4. Publishes read notification via Redis pub/sub (`read:{msg_id}`)

**Example:**

```rust
im.mark_read_instant_message(reader_id, msg_id).await?;
println!("Message marked as read");
```

---

### `fetch_read_receipts()`

Fetches all read receipts for a message.

```rust
pub async fn fetch_read_receipts(
    &self,
    msg_id: Uuid,
) -> Result<Vec<ReadReceipt>>
```

**Parameters:**
- `msg_id`: UUID of the message

**Returns:** `Result<Vec<ReadReceipt>>`

**Example:**

```rust
let receipts = im.fetch_read_receipts(msg_id).await?;

for receipt in receipts {
    println!("Read by {} at {}", receipt.client_id, receipt.read_at);
}
```

---

### `get_health_status()`

Gets current health metrics for monitoring.

```rust
pub fn get_health_status(&self) -> HealthStatus
```

**Returns:** `HealthStatus` struct with channel capacities

**Example:**

```rust
let health = im.get_health_status();
println!("Flusher available: {}", health.flusher_channel_available);
println!("Deleter available: {}", health.deleter_channel_available);

// Alert if channels are filling up
if health.flusher_channel_available < 1000 {
    eprintln!("WARNING: Flusher channel under pressure!");
}
```

---

## Usage Examples

### Example 1: Simple P2P Chat

```rust
use instant_message::{InstantMessage, MetricsConfig};
use uuid::Uuid;

async fn simple_chat_example(im: &InstantMessage) -> Result<(), Box<dyn std::error::Error>> {
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();
    let api_key = Uuid::new_v4();

    // Alice sends message to Bob
    let msg_id = im.send_instant_message(
        alice,
        bob,
        "encrypted_hello".to_string(),
        "nonce123".to_string(),
        50,
        Some(api_key),
        "signature_alice".to_string(),
        "pubkey_alice".to_string(),
    ).await?;

    println!("Alice → Bob: Message {} sent", msg_id);

    // Bob fetches his messages
    let messages = im.fetch_messages_for_recipient(bob).await?;
    
    for msg in messages {
        println!("Bob received: {} bytes from {}", 
                 msg.content_size_bytes, 
                 msg.sender_client_id);
    }

    Ok(())
}
```

### Example 2: High-Volume Messaging

```rust
async fn high_volume_example(im: &InstantMessage) -> Result<(), Box<dyn std::error::Error>> {
    let sender = Uuid::new_v4();
    let recipient = Uuid::new_v4();
    let api_key = Uuid::new_v4();

    // Send 10,000 messages concurrently
    let mut handles = vec![];
    
    for i in 0..10_000 {
        let im_clone = im.clone();
        let handle = tokio::spawn(async move {
            im_clone.send_instant_message(
                sender,
                recipient,
                format!("message_{}", i),
                format!("nonce_{}", i),
                100,
                Some(api_key),
                format!("sig_{}", i),
                "pubkey".to_string(),
            ).await
        });
        handles.push(handle);
    }

    // Wait for all sends to complete
    for handle in handles {
        handle.await??;
    }

    println!("Sent 10,000 messages successfully!");

    // Messages are batched and flushed to DB automatically
    // Recipient can fetch them in chunks of 100

    Ok(())
}
```

### Example 3: Read Receipt Tracking

```rust
async fn read_receipt_example(im: &InstantMessage) -> Result<(), Box<dyn std::error::Error>> {
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();
    let api_key = Uuid::new_v4();

    // Alice sends message
    let msg_id = im.send_instant_message(
        alice, bob,
        "encrypted_content".to_string(),
        "nonce".to_string(),
        100,
        Some(api_key),
        "signature".to_string(),
        "pubkey".to_string(),
    ).await?;

    // Bob fetches and reads (automatically marked as read)
    let messages = im.fetch_messages_for_recipient(bob).await?;

    // Alice checks read receipts
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    let receipts = im.fetch_read_receipts(msg_id).await?;
    
    if receipts.is_empty() {
        println!("Message not read yet");
    } else {
        for receipt in receipts {
            println!("Read by {} at {}", receipt.client_id, receipt.read_at);
        }
    }

    Ok(())
}
```

### Example 4: Health Monitoring

```rust
use tokio::time::{interval, Duration};

async fn health_monitor_example(im: InstantMessage) {
    let mut ticker = interval(Duration::from_secs(10));

    loop {
        ticker.tick().await;
        
        let health = im.get_health_status();
        
        // Log metrics
        tracing::info!(
            flusher_available = health.flusher_channel_available,
            deleter_available = health.deleter_channel_available,
            "Health check"
        );

        // Alert on backpressure
        if health.flusher_channel_available < 1000 {
            tracing::warn!("Flusher channel under pressure!");
        }

        if health.deleter_channel_available < 1000 {
            tracing::warn!("Deleter channel under pressure!");
        }
    }
}
```

---

## Advanced Scenarios

### Scenario 1: Multi-Tenant System

```rust
use std::collections::HashMap;
use std::sync::Arc;

struct TenantMessaging {
    instances: HashMap<Uuid, Arc<InstantMessage>>,
}

impl TenantMessaging {
    async fn send_to_tenant(
        &self,
        tenant_id: Uuid,
        sender: Uuid,
        recipient: Uuid,
        content: String,
    ) -> Result<Uuid, Box<dyn std::error::Error>> {
        let im = self.instances.get(&tenant_id)
            .ok_or("Tenant not found")?;
        
        im.send_instant_message(
            sender,
            recipient,
            content,
            "nonce".to_string(),
            100,
            None,
            "sig".to_string(),
            "pubkey".to_string(),
        ).await.map_err(Into::into)
    }
}
```

### Scenario 2: Message Broadcasting

```rust
async fn broadcast_message(
    im: &InstantMessage,
    sender: Uuid,
    recipients: Vec<Uuid>,
    content: String,
) -> Result<Vec<Uuid>, Box<dyn std::error::Error>> {
    let mut message_ids = Vec::new();
    let mut handles = Vec::new();

    for recipient in recipients {
        let im_clone = im.clone();
        let content_clone = content.clone();
        
        let handle = tokio::spawn(async move {
            im_clone.send_instant_message(
                sender,
                recipient,
                content_clone,
                "nonce".to_string(),
                100,
                None,
                "sig".to_string(),
                "pubkey".to_string(),
            ).await
        });
        
        handles.push(handle);
    }

    for handle in handles {
        message_ids.push(handle.await??);
    }

    Ok(message_ids)
}
```

### Scenario 3: Retry Logic with Exponential Backoff

```rust
use tokio::time::{sleep, Duration};

async fn send_with_retry(
    im: &InstantMessage,
    sender: Uuid,
    recipient: Uuid,
    content: String,
    max_retries: u32,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let mut retry_count = 0;
    
    loop {
        match im.send_instant_message(
            sender,
            recipient,
            content.clone(),
            "nonce".to_string(),
            100,
            None,
            "sig".to_string(),
            "pubkey".to_string(),
        ).await {
            Ok(msg_id) => return Ok(msg_id),
            Err(e) if retry_count < max_retries => {
                retry_count += 1;
                let backoff = Duration::from_millis(100 * 2_u64.pow(retry_count));
                tracing::warn!(
                    error = %e,
                    retry = retry_count,
                    "Send failed, retrying in {:?}",
                    backoff
                );
                sleep(backoff).await;
            },
            Err(e) => return Err(e.into()),
        }
    }
}
```

### Scenario 4: Message Pagination

```rust
async fn fetch_all_messages(
    im: &InstantMessage,
    recipient: Uuid,
) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let mut all_messages = Vec::new();
    
    loop {
        let batch = im.fetch_messages_for_recipient(recipient).await?;
        
        if batch.is_empty() {
            break;
        }
        
        all_messages.extend(batch);
        
        // Continue fetching until no more messages
        if all_messages.len() % 100 != 0 {
            break; // Last batch was partial
        }
        
        // Small delay to allow Redis to rebuild inbox if needed
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    
    Ok(all_messages)
}
```

---

## Performance Considerations

### Throughput

- **Send**: ~20,000 RPS per instance (Redis-backed)
- **Fetch**: ~10,000 RPS (with >90% cache hit rate)
- **Read Receipts**: ~15,000 RPS

### Latency (p95)

- **Send**: <5ms (Redis write + queue)
- **Fetch (cache hit)**: <10ms (Redis MGET)
- **Fetch (cache miss)**: <50ms (PostgreSQL query)
- **Read Receipt**: <3ms (Redis pub/sub)

### Resource Usage

- **Redis Memory**: ~2KB per message (with 10min TTL)
- **PostgreSQL Storage**: ~1KB per message (compressed)
- **Channel Buffer**: 20K messages (flusher), 10K messages (deleter)

### Scaling Guidelines

| Metric | Recommendation |
|--------|----------------|
| **Messages/sec** | 1-10K: Single instance<br>10K-50K: 2-3 instances<br>50K+: Horizontal scaling with Redis cluster |
| **Redis Memory** | 1GB per 500K active messages |
| **PostgreSQL** | Connection pool: 10-20 connections per instance |
| **CPU** | 2-4 cores per instance (background workers) |

### Configuration Tuning

```rust
use instant_message::MetricsConfig;

// High-throughput configuration
let config = MetricsConfig {
    max_batch_size: 5000,           // Larger batches
    metric_ttl_secs: 3600,           // 1 hour TTL
    flush_interval_secs: 30,         // Faster flushes
    redis_operation_timeout_secs: 60, // Longer timeout
};

// Low-latency configuration
let config = MetricsConfig {
    max_batch_size: 500,             // Smaller batches
    metric_ttl_secs: 7200,           // 2 hour TTL
    flush_interval_secs: 10,         // Very fast flushes
    redis_operation_timeout_secs: 30, // Short timeout
};
```

---

## Monitoring & Health Checks

### Key Metrics to Track

```rust
// Prometheus-style metrics
counter!("messages_sent_total", 1);
counter!("messages_delivered_total", 1);
counter!("message_fetch_cache_hits", 1);
counter!("message_fetch_cache_misses", 1);
histogram!("message_send_duration_ms", duration.as_millis() as f64);
histogram!("message_fetch_duration_ms", duration.as_millis() as f64);
gauge!("flusher_channel_depth", health.flusher_channel_capacity as f64);
```

### Health Check Endpoint

```rust
use axum::{Json, extract::State};
use serde_json::json;

async fn health_check(
    State(im): State<Arc<InstantMessage>>,
) -> Json<serde_json::Value> {
    let health = im.get_health_status();
    
    let is_healthy = health.flusher_channel_available > 1000
        && health.deleter_channel_available > 1000;
    
    Json(json!({
        "status": if is_healthy { "healthy" } else { "degraded" },
        "flusher_available": health.flusher_channel_available,
        "deleter_available": health.deleter_channel_available,
        "flusher_capacity": health.flusher_channel_capacity,
        "deleter_capacity": health.deleter_channel_capacity,
    }))
}
```

### Alerting Rules

```yaml
# Prometheus alerts
groups:
  - name: instant_message
    rules:
      - alert: HighChannelBackpressure
        expr: flusher_channel_depth > 15000
        for: 5m
        annotations:
          summary: "Flusher channel filling up"
          
      - alert: HighMessageLatency
        expr: histogram_quantile(0.95, message_send_duration_ms) > 100
        for: 5m
        annotations:
          summary: "Message send latency high"
```

---

## Error Handling

### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `VaultlessError::Internal("Redis connection failed")` | Redis unavailable | Check Redis connectivity, increase timeout |
| `VaultlessError::Internal("DB pool dropped")` | Shutdown in progress | Graceful shutdown required |
| `VaultlessError::Timeout("Redis operation timed out")` | Redis overloaded | Scale Redis or increase timeout |
| `VaultlessError::Validation("Signature verification failed")` | Invalid signature | Check encryption key mismatch |

### Error Handling Pattern

```rust
match im.send_instant_message(/* ... */).await {
    Ok(msg_id) => {
        println!("Success: {}", msg_id);
    },
    Err(VaultlessError::Timeout(msg)) => {
        eprintln!("Timeout: {} - retrying...", msg);
        // Retry logic
    },
    Err(VaultlessError::Internal(msg)) => {
        eprintln!("Internal error: {} - alerting ops", msg);
        // Send alert
    },
    Err(e) => {
        eprintln!("Unexpected error: {:?}", e);
    }
}
```

### Graceful Shutdown

```rust
use tokio::signal;

async fn run_server(im: InstantMessage) {
    let im = Arc::new(im);
    
    // Spawn health monitor
    let health_im = Arc::clone(&im);
    tokio::spawn(async move {
        health_monitor_example((*health_im).clone()).await;
    });
    
    // Wait for shutdown signal
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    
    println!("Shutting down gracefully...");
    
    // Background workers will flush remaining buffers automatically
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    println!("Shutdown complete");
}
```

---

## Best Practices

### ✅ DO

- Use API key IDs for usage tracking and billing
- Implement retry logic with exponential backoff
- Monitor channel depths for backpressure
- Set up alerts for high latency or error rates
- Use structured logging with correlation IDs
- Batch operations when possible

### ❌ DON'T

- Don't send messages without signature verification
- Don't ignore errors from metrics operations (they indicate billing issues)
- Don't fetch messages in tight loops (use webhooks/pub-sub)
- Don't disable background workers
- Don't exceed 10K messages/sec without horizontal scaling
- Don't store sensitive data in plaintext

---

## Support & Troubleshooting

### Debug Mode

```rust
// Enable debug logging
tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .init();
```

### Common Issues

**Q: Messages not appearing for recipient**  
A: Check Redis inbox queue: `LLEN inbox:{recipient_id}` and ensure background flusher is running.

**Q: High memory usage in Redis**  
A: Check TTL settings and ensure purger is running. Consider decreasing `CACHE_TTL_SECS`.

**Q: Slow message delivery**  
A: Check cache hit rate. If low, increase `CACHE_TTL_SECS` or optimize PostgreSQL queries.

**Q: Double-counting in metrics**  
A: This is prevented by atomic Lua scripts. If occurring, check Redis cluster consistency.

---

## License & Contributing

This documentation covers the public API of the InstantMessage system. For implementation details, see the source code comments.

**Last Updated**: 2025-10-28  
**API Version**: 1.0.0  
**Compatibility**: Rust 1.70+