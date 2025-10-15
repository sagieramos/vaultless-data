# 🔐 Vaultless Data API Documentation

**Version:** 0.1.0  
**Base URL:** `http://localhost:8080`  
**Production URL:** `https://api.vaultless.io` *(coming soon)*

End-to-end encrypted message relay with zero-knowledge architecture. The backend never sees your plaintext data or encryption keys.

---

## 📋 Table of Contents

- [Authentication](#authentication)
- [Health & Status](#health--status)
- [Admin Endpoints](#admin-endpoints)
- [Message Endpoints](#message-endpoints)
- [Analytics Endpoints](#analytics-endpoints)
- [Error Handling](#error-handling)
- [Rate Limits](#rate-limits)
- [Examples](#examples)

---

## 🔑 Authentication

All authenticated endpoints require an API key in the `Authorization` header.

### Header Format

```
Authorization: vlt_your_api_key_here
```

**OR**

```
Authorization: Bearer vlt_your_api_key_here
```

### Getting an API Key

Use the [Create API Key](#1-create-api-key) endpoint (development only).

---

## 💚 Health & Status

### 1. Health Check

**GET** `/health`

Check if the API and database are healthy.

**Response:**
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "database": {
    "connected": true,
    "pool_size": 10
  }
}
```

**Status Codes:**
- `200 OK` - Service is healthy
- `503 Service Unavailable` - Database connection failed

---

### 2. Readiness Check

**GET** `/ready`

Kubernetes-friendly readiness probe.

**Response:** Empty body  
**Status Codes:**
- `200 OK` - Ready to serve traffic
- `503 Service Unavailable` - Not ready

---

### 3. Liveness Check

**GET** `/live`

Kubernetes-friendly liveness probe.

**Response:** Empty body  
**Status Code:** `200 OK`

---

## 🔧 Admin Endpoints

> ⚠️ **WARNING:** These endpoints have no authentication in development. **DO NOT USE IN PRODUCTION.**

### 1. Create API Key

**POST** `/api/v1/admin/keys/create`

Generate a new API key.

**Request Body:**
```json
{
  "owner_email": "user@example.com",
  "owner_name": "John Doe",
  "organization": "Acme Corp",
  "tier": "pro"
}
```

**Parameters:**
- `owner_email` (optional): Email address
- `owner_name` (optional): Owner's name
- `organization` (optional): Organization name
- `tier` (optional): `free`, `starter`, `pro`, or `enterprise` (default: `free`)

**Response:**
```json
{
  "api_key": "vlt_kX9mN2pQ7wR8sT5uV6xY1zA3bC4dE5fG6h",
  "key_prefix": "vlt_kX9mN2pQ",
  "tier": "pro",
  "monthly_quota": 500000,
  "warning": "SAVE THIS KEY NOW - IT WILL NOT BE SHOWN AGAIN"
}
```

**Status Codes:**
- `201 Created` - API key created successfully
- `400 Bad Request` - Invalid input
- `409 Conflict` - Key already exists

---

### 2. List API Keys

**GET** `/api/v1/admin/keys`

List all API keys (metadata only, no actual keys shown).

**Response:**
```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "key_prefix": "vlt_kX9mN2pQ",
    "tier": "pro",
    "owner_email": "user@example.com",
    "is_active": true,
    "created_at": "2025-10-15T10:00:00Z",
    "last_used_at": "2025-10-15T10:30:00Z"
  }
]
```

---

## 📨 Message Endpoints

### 1. Send Message

**POST** `/api/v1/messages/send`

Send an encrypted message to a recipient.

**Authentication:** Required

**Request Body:**
```json
{
  "recipient_id": "user@example.com",
  "ciphertext": "YmFzZTY0X2VuY3J5cHRlZF9kYXRhX2hlcmU=",
  "nonce": "cmFuZG9tX25vbmNlXzEyMzQ1",
  "content_type": "text/plain",
  "content_size_bytes": 1024,
  "ttl_seconds": 86400,
  "max_access_count": 3,
  "require_proof_verification": false
}
```

**Parameters:**
- `recipient_id` (required): Recipient identifier (email, user ID, etc.)
- `ciphertext` (required): Base64-encoded encrypted data
- `nonce` (required): Base64-encoded nonce (12 bytes for AES-GCM)
- `content_type` (optional): MIME type (default: `application/octet-stream`)
- `content_size_bytes` (required): Original content size in bytes
- `ttl_seconds` (optional): Time-to-live in seconds (overrides tier default)
- `max_access_count` (optional): Max times message can be accessed before deletion
- `require_proof_verification` (optional): Require cryptographic proof (default: `false`)

**Response:**
```json
{
  "message_id": "650e8400-e29b-41d4-a716-446655440001",
  "recipient_id": "user@example.com",
  "expires_at": "2025-10-16T10:00:00Z",
  "created_at": "2025-10-15T10:00:00Z"
}
```

**Status Codes:**
- `201 Created` - Message sent successfully
- `400 Bad Request` - Invalid input
- `401 Unauthorized` - Invalid or missing API key
- `429 Too Many Requests` - Quota exceeded or rate limited

---

### 2. Receive Messages

**GET** `/api/v1/messages/:recipient_id`

Retrieve all undelivered messages for a recipient.

**Authentication:** Required

**Path Parameters:**
- `recipient_id`: Recipient identifier

**Response:**
```json
{
  "total_count": 2,
  "messages": [
    {
      "id": "650e8400-e29b-41d4-a716-446655440001",
      "ciphertext": "YmFzZTY0X2VuY3J5cHRlZF9kYXRhX2hlcmU=",
      "nonce": "cmFuZG9tX25vbmNlXzEyMzQ1",
      "content_type": "text/plain",
      "content_size_bytes": 1024,
      "created_at": "2025-10-15T10:00:00Z",
      "expires_at": "2025-10-16T10:00:00Z",
      "access_count": 1,
      "max_access_count": 3
    }
  ]
}
```

**Status Codes:**
- `200 OK` - Messages retrieved successfully
- `401 Unauthorized` - Invalid or missing API key
- `404 Not Found` - No messages found

**Notes:**
- Messages are automatically marked as accessed
- Cached for 60 seconds for performance
- Messages with `max_access_count` will be deleted after limit

---

### 3. Get Message Metadata

**GET** `/api/v1/messages/:message_id/metadata`

Get message metadata without the ciphertext.

**Authentication:** Required

**Path Parameters:**
- `message_id`: Message UUID

**Response:**
```json
{
  "id": "650e8400-e29b-41d4-a716-446655440001",
  "recipient_id": "user@example.com",
  "content_type": "text/plain",
  "content_size_bytes": 1024,
  "created_at": "2025-10-15T10:00:00Z",
  "expires_at": "2025-10-16T10:00:00Z",
  "access_count": 1,
  "max_access_count": 3
}
```

**Status Codes:**
- `200 OK` - Metadata retrieved successfully
- `400 Bad Request` - Invalid message ID format
- `401 Unauthorized` - Invalid or missing API key
- `403 Forbidden` - Access denied to this message
- `404 Not Found` - Message not found

---

## 📊 Analytics Endpoints

Powered by TimescaleDB for lightning-fast queries! ⚡

### 1. Analytics Dashboard

**GET** `/api/v1/analytics/dashboard`

Get comprehensive usage analytics for your API key.

**Authentication:** Required

**Response:**
```json
{
  "current_month": {
    "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
    "total_messages_sent": 42,
    "total_messages_received": 38,
    "total_proofs_verified": 0,
    "total_bytes_stored": 43008,
    "total_rate_limit_hits": 0,
    "total_estimated_cost_cents": 0
  },
  "last_7_days": [
    {
      "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
      "day": "2025-10-15T00:00:00Z",
      "total_messages_sent": 12,
      "total_messages_received": 10,
      "total_proofs_verified": 0,
      "total_bytes_stored": 12288,
      "total_rate_limit_hits": 0,
      "total_estimated_cost_cents": 0
    }
  ],
  "last_4_weeks": [
    {
      "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
      "week_start": "2025-10-07T00:00:00Z",
      "total_messages_sent": 42,
      "total_messages_received": 38,
      "total_proofs_verified": 0,
      "total_bytes_stored": 43008,
      "total_rate_limit_hits": 0,
      "total_estimated_cost_cents": 0
    }
  ],
  "quota_usage": {
    "monthly_quota": 500000,
    "messages_used": 42,
    "percentage_used": 0.0084,
    "remaining": 499958,
    "will_exceed": false
  },
  "trends": {
    "current_week": 42,
    "previous_week": 0,
    "change_percent": 100.0,
    "trend": "up"
  }
}
```

**Status Codes:**
- `200 OK` - Dashboard data retrieved successfully
- `401 Unauthorized` - Invalid or missing API key

---

### 2. Daily Usage

**GET** `/api/v1/analytics/daily?start=2025-10-01&end=2025-10-15`

Get daily usage breakdown for a date range.

**Authentication:** Required

**Query Parameters:**
- `start` (required): Start date in ISO 8601 format (YYYY-MM-DD)
- `end` (required): End date in ISO 8601 format (YYYY-MM-DD)

**Response:**
```json
[
  {
    "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
    "day": "2025-10-15T00:00:00Z",
    "total_messages_sent": 12,
    "total_messages_received": 10,
    "total_proofs_verified": 0,
    "total_bytes_stored": 12288,
    "total_rate_limit_hits": 0,
    "total_estimated_cost_cents": 0
  }
]
```

**Status Codes:**
- `200 OK` - Daily usage retrieved successfully
- `400 Bad Request` - Invalid date format
- `401 Unauthorized` - Invalid or missing API key

---

### 3. Weekly Usage

**GET** `/api/v1/analytics/weekly?start=2025-09-01&end=2025-10-15`

Get weekly usage breakdown for a date range.

**Authentication:** Required

**Query Parameters:**
- `start` (required): Start date in ISO 8601 format (YYYY-MM-DD)
- `end` (required): End date in ISO 8601 format (YYYY-MM-DD)

**Response:**
```json
[
  {
    "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
    "week_start": "2025-10-07T00:00:00Z",
    "total_messages_sent": 42,
    "total_messages_received": 38,
    "total_proofs_verified": 0,
    "total_bytes_stored": 43008,
    "total_rate_limit_hits": 0,
    "total_estimated_cost_cents": 0
  }
]
```

**Status Codes:**
- `200 OK` - Weekly usage retrieved successfully
- `400 Bad Request` - Invalid date format
- `401 Unauthorized` - Invalid or missing API key

---

## ❌ Error Handling

All errors follow a consistent JSON format:

```json
{
  "error": {
    "message": "Invalid API key",
    "status": 401
  }
}
```

**Or with error code:**

```json
{
  "error": {
    "message": "Monthly message quota exceeded",
    "code": "QUOTA_EXCEEDED",
    "status": 429
  }
}
```

### Common HTTP Status Codes

- `200 OK` - Request succeeded
- `201 Created` - Resource created successfully
- `400 Bad Request` - Invalid input or malformed request
- `401 Unauthorized` - Invalid or missing API key
- `403 Forbidden` - Valid API key but access denied
- `404 Not Found` - Resource not found
- `409 Conflict` - Resource already exists
- `422 Unprocessable Entity` - Validation error
- `429 Too Many Requests` - Rate limit or quota exceeded
- `500 Internal Server Error` - Server error (contact support)
- `503 Service Unavailable` - Service temporarily unavailable

---

## 🚦 Rate Limits

Rate limits are enforced per API key based on your subscription tier.

### Current Limits (per minute)

| Tier | Requests/Min | Monthly Messages | Retention |
|------|--------------|------------------|-----------|
| **Free** | 60 | 1,000 | 7 days |
| **Starter** | 300 | 50,000 | 30 days |
| **Pro** | 1,000 | 500,000 | 90 days |
| **Enterprise** | 10,000 | Unlimited | 365 days |

### Rate Limit Headers

Responses include rate limit information:

```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1697385600
```

---

## 📚 Examples

### Postman Collection

Import this collection to test all endpoints:

**Collection Name:** Vaultless Data API

**Variables:**
- `base_url`: `http://localhost:8080`
- `api_key`: `your_api_key_here`

---

### Example 1: Complete Message Flow

#### Step 1: Create an API Key

```bash
curl -X POST http://localhost:8080/api/v1/admin/keys/create \
  -H "Content-Type: application/json" \
  -d '{
    "owner_email": "alice@example.com",
    "owner_name": "Alice",
    "tier": "pro"
  }'
```

**Save the `api_key` from the response!**

---

#### Step 2: Send a Message

```bash
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: vlt_YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "bob@example.com",
    "ciphertext": "YmFzZTY0X2VuY3J5cHRlZF9kYXRhX2hlcmU=",
    "nonce": "cmFuZG9tX25vbmNlXzEyMzQ1",
    "content_size_bytes": 1024,
    "ttl_seconds": 86400
  }'
```

---

#### Step 3: Receive Messages

```bash
curl http://localhost:8080/api/v1/messages/bob@example.com \
  -H "Authorization: vlt_YOUR_API_KEY"
```

---

#### Step 4: Check Analytics

```bash
curl http://localhost:8080/api/v1/analytics/dashboard \
  -H "Authorization: vlt_YOUR_API_KEY"
```

---

### Example 2: Self-Destructing Message

Send a message that deletes itself after 3 reads:

```bash
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: vlt_YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "secure@example.com",
    "ciphertext": "c2VjcmV0X2RhdGFfaGVyZQ==",
    "nonce": "c2VjdXJlX25vbmNl",
    "content_size_bytes": 512,
    "max_access_count": 3,
    "ttl_seconds": 3600
  }'
```

---

### Example 3: Get Historical Usage

Get daily usage for the last 30 days:

```bash
curl "http://localhost:8080/api/v1/analytics/daily?start=2025-09-15&end=2025-10-15" \
  -H "Authorization: vlt_YOUR_API_KEY"
```

---

## 🔐 Security Best Practices

### API Keys
- ✅ Store API keys securely (environment variables, secrets manager)
- ✅ Never commit API keys to version control
- ✅ Rotate keys regularly
- ✅ Use different keys for development and production
- ❌ Never expose API keys in client-side code

### Encryption
- ✅ Encrypt data client-side before sending
- ✅ Use AES-256-GCM for encryption
- ✅ Generate unique nonces for each message
- ✅ Store encryption keys securely (never send to API)
- ❌ Never send plaintext data to the API

### Messages
- ✅ Set appropriate TTL values
- ✅ Use `max_access_count` for sensitive data
- ✅ Delete messages after retrieval when possible
- ✅ Validate recipient identifiers
- ❌ Don't store sensitive data longer than necessary

---

## 🆘 Support

- **Documentation:** https://docs.vaultless.io *(coming soon)*
- **GitHub:** https://github.com/yourusername/vaultless-data
- **Email:** support@vaultless.io *(coming soon)*

---

## 📝 Changelog

### v0.1.0 (2025-10-15)
- Initial release
- Message send/receive endpoints
- Analytics dashboard with TimescaleDB
- Daily and weekly usage rollups
- API key management
- Health checks

---

**Built with ❤️ using Rust, Axum, PostgreSQL, TimescaleDB, and Dragonfly**