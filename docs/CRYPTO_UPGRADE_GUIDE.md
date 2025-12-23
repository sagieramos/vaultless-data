# Cryptographic Architecture Upgrade Guide

## Overview

This document describes the upgrade from single-key to dual-key cryptography with enhanced encryption algorithms.

### Key Changes

1. **Dual-Key Architecture**: Separate Ed25519 (signing) and X25519 (key exchange) keys
2. **Forward Secrecy**: Ephemeral session keys via ECDH
3. **Modern Encryption**: XChaCha20-Poly1305 replaces AES-256-GCM
4. **Handshake Protocol**: Signed key exchange with session establishment

---

## Architecture

### Before (Legacy)
```
┌─────────────────────┐
│  Client             │
│  - Ed25519 key      │  (Used for both signing and identity)
│  - Static key       │  (No forward secrecy)
│  - AES-256-GCM      │  (12-byte nonce)
└─────────────────────┘
```

### After (Current)
```
┌──────────────────────────────────┐
│  Client                          │
│  ┌──────────────────────────┐   │
│  │ Identity Keys (Static)   │   │
│  │  - Ed25519 signing_key   │   │  (Authentication)
│  │  - X25519 exchange_key   │   │  (Key agreement)
│  └──────────────────────────┘   │
│  ┌──────────────────────────┐   │
│  │ Session Keys (Ephemeral) │   │
│  │  - X25519 ephemeral key  │   │  (Per-session)
│  │  - Derived session key   │   │  (HKDF-SHA256)
│  │  - XChaCha20-Poly1305    │   │  (24-byte nonce)
│  └──────────────────────────┘   │
└──────────────────────────────────┘
```

---

## Cryptographic Primitives

### Key Generation

#### 1. Signing Keypair (Ed25519)
```rust
use vaultless_core::crypto::generate_signing_keypair;

let signing_keypair = generate_signing_keypair()?;
// signing_keypair.private_key - 32 bytes, base64
// signing_keypair.public_key  - 32 bytes, base64
```

**Purpose**: Authentication, signature verification
**Storage**: Public key in `clients.signing_key`
**Security**: Keep private key client-side only

#### 2. Exchange Keypair (X25519)
```rust
use vaultless_core::crypto::generate_exchange_keypair;

let exchange_keypair = generate_exchange_keypair()?;
// exchange_keypair.private_key - 32 bytes, base64
// exchange_keypair.public_key  - 32 bytes, base64
```

**Purpose**: ECDH key agreement
**Storage**: Public key in `clients.exchange_key`
**Security**: Keep private key client-side only

#### 3. Dual Keypair (Convenience)
```rust
use vaultless_core::crypto::generate_dual_keypair;

let dual = generate_dual_keypair()?;
// dual.signing   - Ed25519 keypair
// dual.exchange  - X25519 keypair
```

---

## Handshake Protocol

### Flow Diagram

```
Alice (Initiator)                    Bob (Responder)
─────────────────                    ───────────────

1. Generate ephemeral X25519 key
   eph_alice = X25519::generate()

2. Create signed request
   request = {
     handshake_id,
     signing_pubkey: alice.ed25519_pub,
     ephemeral_exchange_pubkey: eph_alice.pub,
     timestamp,
     signature: Ed25519::sign(...)
   }

3. Send request ──────────────────────>

                                      4. Verify signature
                                         Check timestamp freshness

                                      5. Generate ephemeral key
                                         eph_bob = X25519::generate()

                                      6. Create signed response
                                         response = {
                                           handshake_id (same),
                                           signing_pubkey: bob.ed25519_pub,
                                           ephemeral_exchange_pubkey: eph_bob.pub,
                                           session_id: UUID,
                                           expires_at,
                                           signature: Ed25519::sign(...)
                                         }

                            <────────────── 7. Send response

8. Verify signature
   Check handshake_id matches

9. Perform ECDH
   shared_secret = ECDH(
     eph_alice.priv,
     eph_bob.pub
   )

10. Derive session key
    session_key = HKDF-SHA256(
      shared_secret,
      salt: session_id,
      info: "vaultless-session-v1"
    )

                                      11. Perform same ECDH
                                          shared_secret = ECDH(
                                            eph_bob.priv,
                                            eph_alice.pub
                                          )

                                      12. Derive same session key
                                          session_key = HKDF-SHA256(...)

✓ Both have same session_key
✓ Forward secrecy achieved
✓ Ready to exchange encrypted messages
```

### Code Example

```rust
use vaultless_core::crypto::handshake::*;
use vaultless_core::crypto::keys::*;

// === Alice (Initiator) ===
let alice_signing = generate_signing_keypair()?;
let alice_ephemeral = generate_exchange_keypair()?;

// Step 1: Create handshake request
let request = create_handshake_request(
    &alice_signing,
    &alice_ephemeral.public_key,
)?;

// Send request to Bob...

// === Bob (Responder) ===
let bob_signing = generate_signing_keypair()?;
let bob_ephemeral = generate_exchange_keypair()?;

// Step 2: Respond to handshake
let response = respond_to_handshake(
    &request,
    &bob_signing,
    &bob_ephemeral.public_key,
    60, // Session duration in minutes
)?;

// Send response to Alice...

// === Alice ===
// Step 3: Complete handshake
let alice_result = complete_handshake(
    &response,
    &alice_ephemeral.private_key,
    &request.handshake_id,
)?;

// === Bob ===
// Step 4: Derive session key
let bob_result = derive_responder_session_key(
    &response,
    &bob_ephemeral.private_key,
    &request.ephemeral_exchange_pubkey,
)?;

// Both now have session_key for encryption
assert_eq!(alice_result.session_key, bob_result.session_key);
```

---

## Encryption

### XChaCha20-Poly1305

#### Advantages over AES-256-GCM

1. **Extended Nonce**: 24 bytes vs 12 bytes (better collision resistance)
2. **No AES Hardware Requirement**: Pure software implementation
3. **Constant Time**: Resistant to timing attacks
4. **Better for Random Nonces**: Larger nonce space = safer random generation

#### Encryption Example

```rust
use vaultless_core::crypto::encryption::{encrypt_xchacha, decrypt_xchacha};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

// Encrypt
let plaintext = b"Secret message";
let session_key_vec = BASE64.decode(&session_key)?;
let mut key: [u8; 32] = session_key_vec.try_into()?;

let encrypted = encrypt_xchacha(plaintext, &mut key)?;
// encrypted.ciphertext - Base64-encoded
// encrypted.nonce      - Base64-encoded (24 bytes)

// Decrypt
let session_key_vec = BASE64.decode(&session_key)?;
let mut key: [u8; 32] = session_key_vec.try_into()?;

let decrypted = decrypt_xchacha(&encrypted, &mut key)?;
```

#### Algorithm Versioning

Messages now include algorithm metadata for forward compatibility:

```rust
pub struct Message {
    // ...
    encryption_algorithm: String,  // "xchacha20-poly1305" or "aes-256-gcm"
    algorithm_version: i16,         // 1 for current
}
```

---

## Database Schema Changes

### Migration: `20251222000000_crypto_upgrade_dual_keys.sql`

#### Clients Table

**Before:**
```sql
CREATE TABLE clients (
    id UUID PRIMARY KEY,
    public_key TEXT UNIQUE,  -- Single Ed25519 key
    ...
);
```

**After:**
```sql
CREATE TABLE clients (
    id UUID PRIMARY KEY,
    signing_key TEXT UNIQUE,    -- Ed25519 for authentication
    exchange_key TEXT UNIQUE,   -- X25519 for key agreement
    ...
);
```

#### Session Keys Table (New)

```sql
CREATE TABLE session_keys (
    id UUID PRIMARY KEY,
    client_id UUID REFERENCES clients(id),
    peer_client_id UUID REFERENCES clients(id),

    session_id VARCHAR(64) UNIQUE,
    ephemeral_public_key TEXT NOT NULL,  -- Ephemeral X25519

    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    last_used_at TIMESTAMPTZ,

    messages_sent BIGINT DEFAULT 0,
    messages_received BIGINT DEFAULT 0,

    is_active BOOLEAN DEFAULT true,

    UNIQUE (client_id, peer_client_id, is_active)
);
```

**Purpose**: Track ephemeral sessions for forward secrecy

#### Messages Table (Updated)

```sql
ALTER TABLE messages
ADD COLUMN encryption_algorithm VARCHAR(32) DEFAULT 'aes-256-gcm',
ADD COLUMN algorithm_version SMALLINT DEFAULT 1;
```

---

## Migration Strategy

### Phase 1: Database Migration ✅

Run the migration:
```bash
sqlx migrate run
```

### Phase 2: Client Updates (Next Steps)

#### Update Client Model

File: `vaultless-core/src/models/clients/mod.rs`

```rust
pub struct Client {
    pub id: Uuid,
    pub signing_key: Option<String>,      // NEW: Ed25519
    pub exchange_key: Option<String>,     // NEW: X25519
    // Remove: pub public_key
}
```

#### Update Registration DTO

File: `vaultless-core/src/models/clients/dto.rs`

```rust
pub struct CreateClientRequest {
    pub signing_pubkey: String,        // NEW: Ed25519 public key
    pub exchange_pubkey: String,       // NEW: X25519 public key
    pub challenge: String,
    pub challenge_signed: String,      // Signed with Ed25519 private key
}
```

#### Update Authentication

Signature verification should use `signing_key` instead of `public_key`.

### Phase 3: Message Handlers (Next Steps)

#### Add Handshake Endpoint

```rust
POST /clients/handshake/initiate
{
    "peer_client_id": "uuid",
    "ephemeral_exchange_pubkey": "base64",
    "signature": "base64"
}

POST /clients/handshake/respond
{
    "handshake_id": "uuid",
    "ephemeral_exchange_pubkey": "base64",
    "signature": "base64"
}
```

#### Update Message Send

```rust
pub struct SendMessageRequest {
    pub recipient_id: Uuid,
    pub ciphertext: String,
    pub nonce: String,               // Now 24 bytes for XChaCha20
    pub session_id: String,          // NEW: Reference to session
    pub encryption_algorithm: String, // NEW: "xchacha20-poly1305"
    pub signature: String,
}
```

---

## Security Properties

### Forward Secrecy ✅

- Ephemeral X25519 keys generated per session
- Session keys derived from ephemeral ECDH
- Compromise of static keys doesn't reveal past sessions

### Authentication ✅

- Ed25519 signatures on all handshake messages
- Prevents MITM attacks
- Verifies peer identity

### Confidentiality ✅

- XChaCha20-Poly1305 AEAD encryption
- Session keys never transmitted
- Zero-knowledge server (server never decrypts)

### Integrity ✅

- Poly1305 authentication tags on all ciphertext
- Ed25519 signatures on handshake protocol
- Detects tampering

---

## Testing

### Run Crypto Tests

```bash
# All crypto tests
cargo test --lib crypto

# Specific modules
cargo test --lib crypto::handshake
cargo test --lib crypto::key_exchange
cargo test --lib crypto::encryption
cargo test --lib crypto::keys
```

### Test Coverage

- ✅ X25519 key generation
- ✅ ECDH key exchange
- ✅ HKDF session key derivation
- ✅ XChaCha20-Poly1305 encryption/decryption
- ✅ Handshake protocol (5 tests)
- ✅ Signature verification
- ✅ Timestamp validation
- ✅ Tamper detection

---

## API Changes Summary

### New Exports

```rust
// vaultless_core::crypto

// Key types
pub use keys::{
    DualKeypair,
    ExchangeKeypair,
    SigningKeypair,
    generate_dual_keypair,
    generate_exchange_keypair,
};

// Encryption
pub use encryption::{
    EncryptionAlgorithm,
    encrypt_xchacha,
    decrypt_xchacha,
    XCHACHA_NONCE_SIZE,
};

// Key exchange
pub use key_exchange::{
    SESSION_KEY_SIZE,
    perform_key_exchange,
    derive_session_key,
    exchange_and_derive,
};

// Handshake (new module)
pub use handshake::{
    HandshakeRequest,
    HandshakeResponse,
    HandshakeResult,
    create_handshake_request,
    respond_to_handshake,
    complete_handshake,
};
```

---

## Performance Notes

### Benchmarks (Approximate)

| Operation | Time | Notes |
|-----------|------|-------|
| Ed25519 Sign | ~50 μs | Constant time |
| Ed25519 Verify | ~150 μs | Batch verification faster |
| X25519 ECDH | ~40 μs | Single scalar multiplication |
| HKDF-SHA256 | ~5 μs | For 32-byte output |
| XChaCha20 Encrypt | ~0.5 μs/KB | Highly parallelizable |
| AES-256-GCM Encrypt | ~0.3 μs/KB | With hardware acceleration |

### Trade-offs

- **XChaCha20-Poly1305**: Slightly slower than AES-GCM with hardware, but constant time and larger nonce space
- **Handshake**: One-time per session, amortized over many messages
- **Session Storage**: New table adds minimal overhead

---

## Client Implementation Checklist

For client-side (SDK) implementation:

- [ ] Generate dual keypair on client registration
- [ ] Store private keys securely (never transmit)
- [ ] Implement handshake protocol
- [ ] Generate ephemeral keys per session
- [ ] Derive session keys using HKDF
- [ ] Switch to XChaCha20-Poly1305 encryption
- [ ] Include session_id in message metadata
- [ ] Handle session expiry and re-handshake
- [ ] Backward compatibility for old clients (if needed)

---

## Troubleshooting

### Common Issues

**Q: Why are my handshake requests failing?**
A: Check timestamp freshness (must be < 5 minutes old) and signature verification.

**Q: Session keys don't match between Alice and Bob**
A: Ensure you're using the same `session_id` as salt in HKDF. Verify ECDH order: `Alice_private × Bob_public = Bob_private × Alice_public`.

**Q: XChaCha20 nonce size mismatch**
A: XChaCha20 uses 24-byte nonces (XCHACHA_NONCE_SIZE), not 12 bytes like AES-GCM.

**Q: How do I handle legacy clients?**
A: Check `encryption_algorithm` field. Support both "aes-256-gcm" (legacy) and "xchacha20-poly1305" (current) during transition.

---

## Next Steps

### Remaining Implementation Tasks

1. **Update Client Model**: Modify Rust struct and database queries
2. **Create SessionKey Model**: New Rust model for `session_keys` table
3. **Update Registration Flow**: Accept both signing_key and exchange_key
4. **Add Handshake Endpoints**: `/handshake/initiate` and `/handshake/respond`
5. **Update Message Send/Receive**: Include session_id and algorithm version
6. **WebSocket Integration**: Support session-based encryption for real-time messages
7. **Session Management**: Implement session expiry, renewal, and cleanup
8. **Monitoring**: Add metrics for handshake success rate, session duration

### Client SDK Updates

1. Generate dual keypairs on setup
2. Implement handshake protocol
3. Session management (creation, renewal, cleanup)
4. Switch encryption to XChaCha20-Poly1305
5. Handle algorithm negotiation

---

## References

### Cryptographic Specifications

- **Ed25519**: [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032)
- **X25519**: [RFC 7748](https://www.rfc-editor.org/rfc/rfc7748)
- **XChaCha20-Poly1305**: [draft-irtf-cfrg-xchacha](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03)
- **HKDF**: [RFC 5869](https://www.rfc-editor.org/rfc/rfc5869)

### Dependencies

- `ed25519-dalek`: Ed25519 signatures
- `x25519-dalek`: X25519 key agreement
- `chacha20poly1305`: XChaCha20-Poly1305 AEAD
- `hkdf`: HMAC-based key derivation
- `sha2`: SHA-256 hashing

---

## Changelog

### 2025-12-22
- ✅ Implemented dual-key architecture (Ed25519 + X25519)
- ✅ Added XChaCha20-Poly1305 encryption
- ✅ Implemented ECDH key exchange
- ✅ Created handshake protocol with forward secrecy
- ✅ Database migration for dual keys and session storage
- ✅ Comprehensive test coverage
- ✅ Documentation and guide

---

## License

This cryptographic implementation follows industry best practices and uses well-vetted libraries. All cryptographic primitives are from the RustCrypto project and dalek-cryptography.
