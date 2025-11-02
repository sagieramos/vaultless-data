# Vaultless Instant Message API

## Overview

The `InstantMessage` module provides a high-performance, production-ready implementation for peer-to-peer (P2P) instant messaging. It handles message sending, delivery, read receipts, and background persistence/caching using Redis for low-latency operations and PostgreSQL for durability. Key features include:

- **Idempotent metrics tracking** for sent/received messages and bytes stored.
- **Signature verification** on envelopes for proof-of-authenticity.
- **Background tasks** for batch flushing, deletion, and purging to minimize latency.
- **Fallback mechanisms** for cache misses and channel backpressure.
- **Group message support** (with P2P focus).

This API is designed for 20k+ RPS workloads, with atomic Redis operations and batched DB upserts.

### Dependencies
- Redis (caching, queues, metrics).
- PostgreSQL (persistence, receipts).
- Metrics via `MetricsConfig` (from `crate::usage`).

### Crate Features
- Serde serialization for messages.
- SQLx for async DB queries.
- Tracing for logging.

## Public Structs

### `Message`
Represents a P2P instant message.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub ciphertext: String,
    pub nonce: Uuid,
    pub content_type: Option<String>,
    pub content_size_bytes: i32,
    pub api_key_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accessed_at: Option<DateTime<Utc>>,
    pub access_count: i32,
    pub is_delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,
    pub max_access_count: Option<i32>,
    pub require_proof_verification: bool,
    pub sender_client_id: Uuid,
    pub recipient_client_id: Uuid,
    pub group_id: Option<Uuid>,
    pub is_group_message: bool,
    // Non-DB fields (Redis-only)
    pub signature: String,
    pub envelope_public_key: String,
    pub file_id: Option<Uuid>,
}
```

**Fields:** Core message data (ciphertext, metadata) plus non-persisted fields (signature, public key).

**Usage:** Returned by `fetch_messages_for_recipient`; updated in-place on delivery.

### `P2PFile`
Represents an encrypted file attachment.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct P2PFile {
    pub id: Uuid,
    pub message_id: Option<Uuid>,
    pub uploader_client_id: Uuid,
    pub encrypted_filename: String,
    pub encrypted_mime_type: String,
    pub file_size_bytes: i64,
    pub encrypted_file_key: String,
    pub nonce: String,
    pub storage_path: String,
    pub chunk_count: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub download_count: i32,
    pub max_downloads: Option<i32>,
}
```

**Usage:** Linked via `Message::file_id` for attachments.

### `ReadReceipt`
Represents a read confirmation.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ReadReceipt {
    pub id: Uuid,
    pub message_id: Uuid,
    pub client_id: Uuid,
    pub read_at: DateTime<Utc>,
}
```

**Usage:** Returned by `fetch_read_receipts`.

### `HealthStatus`
Channel health for monitoring.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub flusher_channel_capacity: usize,
    pub flusher_channel_available: usize,
    pub deleter_channel_capacity: usize,
    pub deleter_channel_available: usize,
}
```

**Usage:** Returned by `get_health_status` for backpressure detection.

## Public Functions

### `InstantMessage::new`
Initializes the service and spawns background tasks (flusher, deleter, purger).

```rust
pub fn new(redis_pool: RedisPool, db_pool: PgPool, config: MetricsConfig) -> Result<Self>
```

**Arguments:**
- `redis_pool`: Redis connection pool.
- `db_pool`: PostgreSQL connection pool.
- `config`: Metrics configuration (TTL, batch sizes).

**Returns:** `Result<InstantMessage>`.

**Side Effects:** Spawns Tokio tasks for background processing.

### `InstantMessage::send_instant_message`
Sends a message to a recipient, caches in Redis, queues for DB flush, and tracks sent metrics.

```rust
pub async fn send_instant_message(
    &self,
    sender_client_id: Uuid,
    recipient_client_id: Uuid,
    ciphertext: String,
    nonce: Uuid,
    content_size_bytes: i32,
    api_key_id: Uuid,
    signature: String,
    envelope_public_key: String,
    require_proof_verification: bool,
) -> Result<Uuid>
```

**Arguments:**
- `sender_client_id`: Sender's client UUID.
- `recipient_client_id`: Recipient's client UUID.
- `ciphertext`: Encrypted message body.
- `nonce`: Encryption nonce (UUID).
- `content_size_bytes`: Message size in bytes.
- `api_key_id`: API key UUID for billing/metrics.
- `signature`: Envelope signature (base64).
- `envelope_public_key`: Sender's public key (base64).
- `require_proof_verification`: If true, envelope must be signed.

**Returns:** `Result<Uuid>` (message ID).

**Behavior:**
- Idempotent sent metrics via SET NX flag.
- Emergency DB write if flusher channel is full.

**Errors:** Redis/DB failures; channel closed.

### `InstantMessage::mark_read_instant_message`
Marks a message as read, inserts receipt, and publishes via Redis Pub/Sub.

```rust
pub async fn mark_read_instant_message(
    &self,
    reader_client_id: Uuid,
    msg_id: Uuid,
) -> Result<()>
```

**Arguments:**
- `reader_client_id`: Reader's client UUID.
- `msg_id`: Message UUID.

**Returns:** `Result<()>`.

**Behavior:**
- Checks/inserts DB receipt; queues pending if Redis-only.
- Updates `delivered_at` if needed.
- Publishes to `read:{msg_id}` channel.

**Errors:** DB/Redis failures.

### `InstantMessage::fetch_read_receipts`
Fetches all read receipts for a message.

```rust
pub async fn fetch_read_receipts(&self, msg_id: Uuid) -> Result<Vec<ReadReceipt>>
```

**Arguments:**
- `msg_id`: Message UUID.

**Returns:** `Result<Vec<ReadReceipt>>`.

**Behavior:** Queries DB directly.

### `InstantMessage::fetch_messages_for_recipient`
Fetches up to 100 undelivered messages for a recipient; verifies, marks delivered, tracks metrics.

```rust
pub async fn fetch_messages_for_recipient(
    &self,
    recipient_client_id: Uuid,
) -> Result<Vec<Message>>
```

**Arguments:**
- `recipient_client_id`: Recipient's client UUID.

**Returns:** `Result<Vec<Message>>` (delivered messages).

**Behavior:**
- Rebuilds inbox from DB if empty (with lock).
- Bulk fetch from Redis; SQL fallback for misses (parallel).
- Verifies signatures; deletes invalids.
- Idempotent received metrics via atomic flag.
- Marks as read; queues deletes.

**Errors:** Redis/DB failures.

### `InstantMessage::get_health_status`
Returns channel health metrics.

```rust
pub fn get_health_status(&self) -> HealthStatus
```

**Returns:** `HealthStatus`.

**Usage:** Monitor for backpressure (e.g., available < threshold).

## Usage Example

```rust
use crate::models::message::InstantMessage;
use deadpool_redis::Pool;
use sqlx::PgPool;
use uuid::Uuid;

// Initialize
let redis_pool = Pool::builder(/* config */).build()?;
let db_pool = PgPool::connect(/* url */).await?;
let metrics_config = MetricsConfig::default();
let im = InstantMessage::new(redis_pool, db_pool, metrics_config)?;

// Send
let msg_id = im.send_instant_message(
    sender_id,
    recipient_id,
    ciphertext,
    nonce,
    size,
    api_key_id,
    signature,
    pub_key,
    true,
).await?;

// Fetch
let messages = im.fetch_messages_for_recipient(recipient_id).await?;

// Read
im.mark_read_instant_message(recipient_id, msg_id).await?;

// Receipts
let receipts = im.fetch_read_receipts(msg_id).await?;

// Health
let health = im.get_health_status();
```

## Configuration Constants

- `CACHE_TTL_SECS`: 600s (Redis expiry).
- `DEFAULT_MESSAGE_EXPIRY_DAYS`: 7.
- `MAX_INBOX_FETCH`: 100.

See source for full list (e.g., batch sizes, intervals).

## Error Handling

- `VaultlessError::Internal`: Redis/DB failures.
- `VaultlessError::Validation`: Invalid keys/signatures.
- Best-effort metrics/logging; core ops fail-fast.

## Internal Behavior

- **Background Tasks:** Flusher (60s batches), Deleter (10s), Purger (1h).
- **Metrics:** Hourly aggregates flushed to `usage_metrics` table.
- **P2P Focus:** Groups handled via flags (no full group API here).

For full source, see `crate::models::message::instant_message`.