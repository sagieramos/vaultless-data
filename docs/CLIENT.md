# Vaultless Client API Documentation
Version: 1.0.0
Base URL: `https://api.vaultless.com` (or `http://localhost:8080` for local development)
Authentication: Bearer Token (Session-based)

___

### 📋 Table of Contents

1. [Overview](#overview)
2. [Authentication Flow](#authentication-flow)
3. [API Endpoints](#api-endpoints)
   - [Public Endpoints](#public-endpoints)
   - [Protected Endpoints](#protected-endpoints)
4. [Security Features](#security-features)
5. [Error Handling](#error-handling)
6. [Code Examples](#code-examples)

___
## Overview
The Vaultless Client API provides a zero-knowledge, anonymous authentication system using cryptographic signatures. Users register with public keys only—no passwords, emails, or personal information required.
### Key Features
- ✅ Passwordless Authentication - Pure cryptographic identity
- ✅ Zero-Knowledge - Server never sees private keys
- ✅ Challenge-Response - Proof of key ownership without exposing secrets
- ✅ Replay Protection - Nonce-based prevention of replay attacks
- ✅ Session Management - 30-day rolling sessions with Redis caching
- ✅ Privacy-First - No PII collection, optional identifiers only
## Authentication Flow
**Registration Flow**
```
sequenceDiagram
    participant Client
    participant Server
    participant Redis
    participant Database

    Client->>Client: Generate Ed25519 keypair
    Client->>Client: Sign payload with private key
    Client->>Server: POST /register (public_key, signature, payload)
    Server->>Server: Verify signature
    Server->>Redis: Check nonce (replay protection)
    Server->>Database: Store public_key + client_hash
    Server->>Redis: Cache client data
    Server->>Client: Return session_token
```
**Authentication Flow**
```
sequenceDiagram
    participant Client
    participant Server
    participant Redis
    participant Database

    Client->>Server: GET /challenge
    Server->>Redis: Store challenge hash (5min TTL)
    Server->>Client: Return challenge string
    Client->>Client: Sign challenge with private key
    Client->>Server: POST /authenticate (identifier, challenge, signature)
    Server->>Redis: Consume challenge (GETDEL)
    Server->>Database: Find client by identifier
    Server->>Server: Verify signature
    Server->>Database: Issue new session token
    Server->>Redis: Invalidate old session
    Server->>Client: Return new session_token
```

## API Endpoints
## Public Endpoints
## 1. Register Client
Create a new anonymous client account with cryptographic identity.

**Endpoint:** `POST /api/clients/register`

**Authentication:** None

**Request Body:**
```json
{
  "public_key": "base64_encoded_public_key",
  "signature": "base64_encoded_signature",
  "signed_payload": "arbitrary_string_that_was_signed",
  "client_identifier": "device_fingerprint_or_unique_id (optional)",
  "identifier": "human_readable_username (optional, 3-64 chars)",
  "nonce": "unique_nonce_for_replay_protection (optional)",
  "timestamp": 1699200000,
  "metadata": {
    "device": "iOS",
    "app_version": "1.0.0"
  }
}
```
**Field Descriptions:**

| Field               | Type    | Required | Description                                                             |
| ------------------- | ------- | -------- | ----------------------------------------------------------------------- |
| `public_key`        | string  | ✅ Yes    | Ed25519/P-256 public key (base64)                                       |
| `signature`         | string  | ✅ Yes    | Signature of `signed_payload` using private key                         |
| `signed_payload`    | string  | ✅ Yes    | Arbitrary string that was signed (e.g., timestamp or client_identifier) |
| `client_identifier` | string  | ❌ No     | Device fingerprint or unique identifier (hashed server-side)            |
| `identifier`        | string  | ❌ No     | Human-readable username (3–64 chars, stored as-is)                      |
| `nonce`             | string  | ❌ No     | Unique nonce for replay protection (8–128 chars)                        |
| `timestamp`         | integer | ❌ No     | Unix timestamp (±60 seconds tolerance)                                  |
| `metadata`          | object  | ❌ No     | Encrypted metadata (device info, preferences)                           |

**Response (200 OK):**
```json
{
  "session_token": "base64_encoded_session_token",
  "expires_at": "2025-12-06T10:30:00Z"
}
```
**Error Responses:**

| Status Code         | Description                                 |
| ------------------- | ------------------------------------------- |
| **400 BAD_REQUEST** | Validation failed (missing required fields) |
| **400 BAD_REQUEST** | Signature verification failed               |
| **400 BAD_REQUEST** | Nonce already used (replay attack)          |
| **400 BAD_REQUEST** | Timestamp outside allowed window            |
| **409 CONFLICT**    | Client already registered                   |


**Example:**
```json
curl -X POST https://api.vaultless.com/api/clients/register \
  -H "Content-Type: application/json" \
  -d '{
    "public_key": "MCowBQYDK2VwAyEA...",
    "signature": "ZXhhbXBsZV9zaWduYXR1cmU...",
    "signed_payload": "test-payload-123",
    "identifier": "alice",
    "nonce": "nonce-1699200000",
    "timestamp": 1699200000
  }'
  ```
  ___
### 2. Generate Challenge
Generate a cryptographic challenge for authentication.<br>
**Endpoint:** `GET /api/clients/challenge` <br>
**Authentication:** None <br>
Response (200 OK):
```json
{
  "challenge": "base64_encoded_random_challenge",
  "expires_at": "2025-11-06T10:35:00Z"
}
```
Notes: <br>
- Challenge is valid for 5 minutes
- Stored in Redis with automatic expiry
- Each challenge is single-use (consumed during authentication) <br>
**Example:**
`curl https://api.vaultless.com/api/clients/challenge`
___
### 3. Authenticate Client
Authenticate using challenge-response or existing session token.

**Endpoint:** `POST /api/clients/authenticate`<br>
**Authentication:** Optional (Bearer token in `Authorization` header)<br>
### Behavior:
1. If valid Authorization: Bearer <token> header present → Returns existing session
2. Otherwise → Performs challenge-based authentication
### Request Body (Challenge-Based Auth):
```json
{
  "client_identifier_hash": "hashed_identifier (optional)",
  "identifier": "alice (optional)",
  "public_key": "base64_public_key (optional)",
  "challenge": "base64_challenge_from_step_2",
  "challenge_signature": "base64_signature_of_challenge"
}
```
**Field Descriptions:**

| Field                                                               | Type   | Required | Description                                 |
| ------------------------------------------------------------------- | ------ | -------- | ------------------------------------------- |
| **One of:** `client_identifier_hash`, `identifier`, or `public_key` | string | ✅        | Client lookup identifier                    |
| **challenge**                                                       | string | ✅        | Challenge string from `/challenge` endpoint |
| **challenge_signature**                                             | string | ✅        | Signature of challenge using private key    |

Response (200 OK - New Session):
```json
{
  "session_token": "new_base64_session_token",
  "expires_at": "2025-12-06T10:30:00Z",
  "is_new_session": true
}
```
Response (200 OK - Existing Session):
```json
{
  "session_token": "",
  "expires_at": "2025-11-25T10:30:00Z",
  "is_new_session": false
}
```
**Error Responses:**

| Status Code          | Description                  |
| -------------------- | ---------------------------- |
| **400 BAD_REQUEST**  | Missing identifier field     |
| **401 UNAUTHORIZED** | Challenge expired or invalid |
| **401 UNAUTHORIZED** | Invalid challenge signature  |
| **401 UNAUTHORIZED** | Client is deactivated        |
| **404 NOT_FOUND**    | Client not found             |

**Example (Challenge-Based):**
```json
curl -X POST https://api.vaultless.com/api/clients/authenticate \
  -H "Content-Type: application/json" \
  -d '{
    "identifier": "alice",
    "challenge": "Y2hhbGxlbmdlX3N0cmluZw==",
    "challenge_signature": "c2lnbmF0dXJlX29mX2NoYWxsZW5nZQ=="
  }'
  ```

**Example (Session Refresh):**
  ```json
  curl -X POST https://api.vaultless.com/api/clients/authenticate \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..." \
  -H "Content-Type: application/json" \
  -d '{}'
  ```
### 4. Lookup Client
Find a client by identifier or public key. <br>
**Endpoint:** `GET /api/clients/lookup` <br>
**Authentication:** None <br>

**Query Parameters:**
| Parameter      | Type   | Required | Description               |
| -------------- | ------ | -------- | ------------------------- |
| **identifier** | string | ❌        | Human-readable identifier |
| **pubkey**     | string | ❌        | Base64-encoded public key |

**Note:** At least one parameter must be provided.

Response (200 OK - Found):

```json
{
  "success": true,
  "client": {
    "identifier": "alice",
    "public_key": "MCowBQYDK2VwAyEA...",
    "allow_anonymous_messages": true,
    "require_proof_verification": false,
    "is_active": true,
    "last_seen_at": "2025-11-06T10:30:00Z",
    "last_message_at": null
  }
}
```

Response (200 OK - Not Found):

```json
{
  "success": false,
  "client": null
}
```

**Example:**

```bash
# Lookup by identifier
curl "https://api.vaultless.com/api/clients/lookup?identifier=alice"

# Lookup by public key
curl "https://api.vaultless.com/api/clients/lookup?pubkey=MCowBQYDK2VwAyEA..."
```
---
### Protected Endpoints

These endpoints require a valid session token in the `Authorization` header.

**Authorization Header Format:**
```
Authorization: Bearer <session_token>
```
### 5. Get Current Client
Retrieve authenticated client information. <br>
**Endpoint:** GET /api/clients/me
**Authentication:** Required
Response (200 OK):
```json
{
  "identifier": "alice",
  "public_key": "MCowBQYDK2VwAyEA...",
  "allow_anonymous_messages": true,
  "require_proof_verification": false,
  "is_active": true,
  "last_seen_at": "2025-11-06T10:30:00Z",
  "last_message_at": "2025-11-05T15:20:00Z"
}
```
**Note:** The id field is not included in the JSON response for privacy.
**Error Responses:**

| Status Code          | Description                      |
| -------------------- | -------------------------------- |
| **401 UNAUTHORIZED** | Missing or invalid session token |
| **401 UNAUTHORIZED** | Session expired                  |

**Example**

```bash
curl https://api.vaultless.com/api/clients/me \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
```
### 6. Logout
Revoke current session.
**Endpoint:** `POST /api/clients/logout` <br>
**Authentication:** Required <br>
**Response (200 OK):**
```json
{
  "success": true,
  "message": "Session revoked successfully"
}
```
**Effects:** <br>
- Clears session token from database
- Invalidates session in Redis cache
- Client must re-authenticate to access protected endpoints

**Example:**
```bash
curl -X POST https://api.vaultless.com/api/clients/logout \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
```
### 7. Deactivate Account
Permanently deactivate client account.
**Endpoint:** DELETE /api/clients/me
**Authentication:** Required
**Response (200 OK):**

```json
{
  "success": true,
  "message": "Client deactivated successfully"
}
```
**Effects:**
- Sets is_active = false in database
- Invalidates all sessions
- Clears all Redis caches (aliases + canonical data)
- Client will not appear in lookup queries
- Account cannot be reactivated (create new account required)

**Example:**
```bash
curl -X DELETE https://api.vaultless.com/api/clients/me \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
```

---

## Security Features

### 1. Signature Verification

All registrations require a valid cryptographic signature:

signature = sign(signed_payload, private_key)
verify(signed_payload, signature, public_key) → must return true

**Supported Algorithms:**
- Ed25519 (recommended)
- ECDSA P-256
- Other curves supported by your crypto library
---
### 2. Challenge-Response Authentication
Prevents session hijacking and proves current key ownership:
1. Client requests challenge: GET /challenge
2. Server generates random 32-byte challenge
3. Server stores challenge hash in Redis (5min TTL)
4. Client signs challenge with private key
5. Client sends (challenge, signature) to /authenticate
6. Server verifies signature using stored public key
7. Server atomically consumes challenge (GETDEL) - single-use only
8. Server issues new session token
   
**Benefits:**
1. Replay Protection: Challenges are single-use
2. Time-Limited: 5-minute expiry window
3. Zero-Knowledge: Private key never transmitted
4. Freshness: Proves current possession of private key
### 3. Nonce-Based Replay Protection
Optional nonce parameter prevents registration replay attacks:
**Recommendations:**
- Use unique nonce per registration attempt
- Format: nonce-<timestamp>-<random>
### 4. Timestamp Freshness Checks
Optional timestamp validation (±60 seconds tolerance):
**Prevents:**
- Old registration requests from being replayed
- Clock skew attacks
### 5. Session Security
- 30-day rolling sessions - Automatic renewal on activity
- Redis caching with hashing - Fast session validation (~100µs)
- Atomic session rotation - Old sessions invalidated on new login
- Secure token generation - 32-byte cryptographically random tokens
- Base64 encoding - URL-safe token transmission
### 6. Privacy Protections

| Stored                          | Not Stored              |
| ------------------------------- | ----------------------- |
| ✅ Public keys                   | ❌ Private keys          |
| ✅ Hashed identifiers            | ❌ Plaintext identifiers |
| ✅ Optional metadata (encrypted) | ❌ Emails, passwords     |
| ✅ Session token hashes          | ❌ IP addresses          |
| ✅ Timestamps                    | ❌ PII of any kind       |


## Error Handling

```json
{
  "error": {
    "message": "Human-readable error message",
    "code": "ERROR_CODE",
    "status": 400
  }
}
```
**Error Codes**

| Code    | HTTP Status    | Description                                 |
| ------- | -------------- | ------------------------------------------- |
| **400** | BAD_REQUEST    | Invalid request format or validation failed |
| **401** | UNAUTHORIZED   | Invalid or expired authentication           |
| **403** | FORBIDDEN      | Operation not allowed for this client       |
| **404** | NOT_FOUND      | Resource not found                          |
| **409** | CONFLICT       | Resource already exists                     |
| **500** | INTERNAL_ERROR | Server error (check logs)                   |

**Common Error Scenarios** <br>
**Registration Errors**

```json
// Missing required field
{
  "error": {
    "message": "public_key is required",
    "code": "BAD_REQUEST",
    "status": 400
  }
}

// Signature verification failed
{
  "error": {
    "message": "Signature verification failed",
    "code": "BAD_REQUEST",
    "status": 400
  }
}

// Nonce already used
{
  "error": {
    "message": "Nonce already used",
    "code": "BAD_REQUEST",
    "status": 400
  }
}

// Duplicate registration
{
  "error": {
    "message": "Client already registered",
    "code": "CONFLICT",
    "status": 409
  }
}
```
**Authentication Errors**
```json
// Invalid challenge
{
  "error": {
    "message": "Invalid or expired challenge",
    "code": "UNAUTHORIZED",
    "status": 401
  }
}

// Invalid signature
{
  "error": {
    "message": "Invalid challenge signature",
    "code": "UNAUTHORIZED",
    "status": 401
  }
}

// Client not found
{
  "error": {
    "message": "Client not found",
    "code": "NOT_FOUND",
    "status": 404
  }
}

// Client deactivated
{
  "error": {
    "message": "Client is deactivated",
    "code": "UNAUTHORIZED",
    "status": 401
  }
}
```

# Code Examples
## JavaScript/TypeScript (Node.js)** <br>
### Generate Keypair and Register**

```typescript
import * as ed from '@noble/ed25519';

// 1. Generate keypair
const privateKey = ed.utils.randomPrivateKey();
const publicKey = await ed.getPublicKey(privateKey);

// 2. Sign payload
const payload = "my-device-fingerprint-123";
const signature = await ed.sign(payload, privateKey);

// 3. Register
const response = await fetch('https://api.vaultless.com/api/clients/register', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    public_key: Buffer.from(publicKey).toString('base64'),
    signature: Buffer.from(signature).toString('base64'),
    signed_payload: payload,
    identifier: 'alice',
    nonce: `nonce-${Date.now()}`,
    timestamp: Math.floor(Date.now() / 1000),
  }),
});

const { session_token, expires_at } = await response.json();

// Store securely
localStorage.setItem('session_token', session_token);
localStorage.setItem('private_key', Buffer.from(privateKey).toString('hex'));
```
### Authenticate with Challenge
```typescript
// 1. Get challenge
const challengeResp = await fetch('https://api.vaultless.com/api/clients/challenge');
const { challenge } = await challengeResp.json();

// 2. Sign challenge
const privateKey = Buffer.from(localStorage.getItem('private_key'), 'hex');
const challengeBytes = Buffer.from(challenge, 'base64');
const signature = await ed.sign(challengeBytes, privateKey);

// 3. Authenticate
const authResp = await fetch('https://api.vaultless.com/api/clients/authenticate', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    identifier: 'alice',
    challenge,
    challenge_signature: Buffer.from(signature).toString('base64'),
  }),
});

const { session_token, is_new_session } = await authResp.json();

if (is_new_session) {
  localStorage.setItem('session_token', session_token);
}
```
## Python
### Generate Keypair and Register
```python
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
import base64
import requests
import time

# 1. Generate keypair
private_key = Ed25519PrivateKey.generate()
public_key = private_key.public_key()

# 2. Sign payload
payload = b"my-device-fingerprint-123"
signature = private_key.sign(payload)

# 3. Register
response = requests.post('https://api.vaultless.com/api/clients/register', json={
    'public_key': base64.b64encode(public_key.public_bytes_raw()).decode(),
    'signature': base64.b64encode(signature).decode(),
    'signed_payload': payload.decode(),
    'identifier': 'alice',
    'nonce': f'nonce-{int(time.time())}',
    'timestamp': int(time.time()),
})

data = response.json()
session_token = data['session_token']

# Store securely
with open('private_key.pem', 'wb') as f:
    from cryptography.hazmat.primitives import serialization
    f.write(private_key.private_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PrivateFormat.PKCS8,
        encryption_algorithm=serialization.NoEncryption()
    ))
```
### Authenticate with Challenge
```python
# 1. Get challenge
challenge_resp = requests.get('https://api.vaultless.com/api/clients/challenge')
challenge = challenge_resp.json()['challenge']

# 2. Load private key and sign
from cryptography.hazmat.primitives import serialization

with open('private_key.pem', 'rb') as f:
    private_key = serialization.load_pem_private_key(f.read(), password=None)

challenge_bytes = base64.b64decode(challenge)
signature = private_key.sign(challenge_bytes)

# 3. Authenticate
auth_resp = requests.post('https://api.vaultless.com/api/clients/authenticate', json={
    'identifier': 'alice',
    'challenge': challenge,
    'challenge_signature': base64.b64encode(signature).decode(),
})

data = auth_resp.json()
if data['is_new_session']:
    session_token = data['session_token']
    # Store securely
```
### Rust
```rust
use ed25519_dalek::{Keypair, Signer, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use reqwest;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Generate keypair
    let mut csprng = rand::rngs::OsRng;
    let keypair = Keypair::generate(&mut csprng);
    
    // 2. Sign payload
    let payload = b"my-device-fingerprint-123";
    let signature = keypair.sign(payload);
    
    // 3. Register
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.vaultless.com/api/clients/register")
        .json(&serde_json::json!({
            "public_key": BASE64.encode(keypair.public.as_bytes()),
            "signature": BASE64.encode(signature.to_bytes()),
            "signed_payload": String::from_utf8_lossy(payload),
            "identifier": "alice",
            "nonce": format!("nonce-{}", chrono::Utc::now().timestamp()),
            "timestamp": chrono::Utc::now().timestamp(),
        }))
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    
    let session_token = response["session_token"].as_str().unwrap();
    println!("Registered! Session: {}", session_token);
    
    Ok(())
}
```
---
### Rate Limiting

| Endpoint       | Limit                          | Window               |
|----------------|--------------------------------|--------------------|
| `/register`    | 10 requests per IP             | per hour            |
| `/challenge`   | 60 requests per IP             | per minute          |
| `/authenticate`| 20 requests per IP             | per minute          |
| `/logout`      | 10 requests per IP             | per minute          |
| `/me`          | 100 requests per IP            | per minute          |

**Note:** Rate limits are subject to change. Contact support for higher limits.
### Support

- Documentation: https://docs.vaultless.com
- GitHub: https://github.com/vaultless/vaultless
- Email: support@vaultless.com
- Discord: https://discord.gg/vaultless


**Changelog** <br>
**v1.0.0 (2025-11-06)**

- Initial release
- Challenge-response authentication
- Nonce-based replay protection
- Session management with Redis caching
- Privacy-first anonymous registration
---
© 2025 Vaultless. All rights reserved.

