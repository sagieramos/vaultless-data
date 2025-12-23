use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{exchange_and_derive, keys, signing};
use crate::error::{Result, VaultlessError};

/// Handshake request from initiator (client A)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequest {
    /// Unique handshake ID
    pub handshake_id: String,

    /// Initiator's Ed25519 signing public key (for authentication)
    pub signing_pubkey: String,

    /// Ephemeral X25519 public key for this session
    pub ephemeral_exchange_pubkey: String,

    /// Timestamp when handshake was initiated
    pub timestamp: DateTime<Utc>,

    /// Ed25519 signature over the handshake payload
    /// Signs: handshake_id || signing_pubkey || ephemeral_exchange_pubkey || timestamp
    pub signature: String,
}

/// Handshake response from responder (client B)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponse {
    /// Reference to the original handshake ID
    pub handshake_id: String,

    /// Responder's Ed25519 signing public key
    pub signing_pubkey: String,

    /// Responder's ephemeral X25519 public key for this session
    pub ephemeral_exchange_pubkey: String,

    /// Timestamp when response was created
    pub timestamp: DateTime<Utc>,

    /// Session ID (to be used as salt in HKDF)
    pub session_id: String,

    /// Session expiry time
    pub expires_at: DateTime<Utc>,

    /// Ed25519 signature over the response payload
    pub signature: String,
}

/// Completed handshake result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResult {
    /// Session ID
    pub session_id: String,

    /// Peer's signing public key
    pub peer_signing_pubkey: String,

    /// Derived session key (base64-encoded, 32 bytes)
    pub session_key: String,

    /// Session expiry
    pub expires_at: DateTime<Utc>,
}

/// Create a handshake request (initiator side)
///
/// # Arguments
/// * `signing_keypair` - Initiator's Ed25519 signing keypair
/// * `ephemeral_exchange_private` - Ephemeral X25519 private key
/// * `ephemeral_exchange_public` - Ephemeral X25519 public key
///
/// # Returns
/// * Signed handshake request ready to send
pub fn create_handshake_request(
    signing_keypair: &keys::SigningKeypair,
    ephemeral_exchange_public: &str,
) -> Result<HandshakeRequest> {
    let handshake_id = Uuid::new_v4().to_string();
    let timestamp = Utc::now();

    // Build payload to sign
    let payload = format!(
        "{}||{}||{}||{}",
        handshake_id,
        signing_keypair.public_key,
        ephemeral_exchange_public,
        timestamp.to_rfc3339()
    );

    // Sign the payload
    let private_key = keys::decode_signing_key(&signing_keypair.private_key)?;
    let signed = signing::sign_data(payload.as_bytes(), &private_key)?;

    Ok(HandshakeRequest {
        handshake_id,
        signing_pubkey: signing_keypair.public_key.clone(),
        ephemeral_exchange_pubkey: ephemeral_exchange_public.to_string(),
        timestamp,
        signature: signed.signature,
    })
}

/// Verify and respond to a handshake request (responder side)
///
/// # Arguments
/// * `request` - Handshake request from initiator
/// * `signing_keypair` - Responder's Ed25519 signing keypair
/// * `ephemeral_exchange_public` - Responder's ephemeral X25519 public key
/// * `session_duration_minutes` - How long the session should last
///
/// # Returns
/// * Signed handshake response
pub fn respond_to_handshake(
    request: &HandshakeRequest,
    signing_keypair: &keys::SigningKeypair,
    ephemeral_exchange_public: &str,
    session_duration_minutes: i64,
) -> Result<HandshakeResponse> {
    // Verify request signature
    verify_handshake_request(request)?;

    // Check timestamp freshness (reject if > 5 minutes old)
    let age = Utc::now().signed_duration_since(request.timestamp);
    if age > Duration::minutes(5) {
        return Err(VaultlessError::Validation(
            "Handshake request expired (> 5 minutes old)".to_string(),
        ));
    }

    // Generate session ID
    let session_id = format!("session-{}", Uuid::new_v4());
    let timestamp = Utc::now();
    let expires_at = timestamp + Duration::minutes(session_duration_minutes);

    // Build response payload to sign
    let payload = format!(
        "{}||{}||{}||{}||{}||{}",
        request.handshake_id,
        signing_keypair.public_key,
        ephemeral_exchange_public,
        timestamp.to_rfc3339(),
        session_id,
        expires_at.to_rfc3339()
    );

    // Sign the response
    let private_key = keys::decode_signing_key(&signing_keypair.private_key)?;
    let signed = signing::sign_data(payload.as_bytes(), &private_key)?;

    Ok(HandshakeResponse {
        handshake_id: request.handshake_id.clone(),
        signing_pubkey: signing_keypair.public_key.clone(),
        ephemeral_exchange_pubkey: ephemeral_exchange_public.to_string(),
        timestamp,
        session_id,
        expires_at,
        signature: signed.signature,
    })
}

/// Complete the handshake (initiator side)
///
/// # Arguments
/// * `response` - Handshake response from responder
/// * `ephemeral_private_key` - Initiator's ephemeral X25519 private key
/// * `expected_handshake_id` - Expected handshake ID (for validation)
///
/// # Returns
/// * HandshakeResult with session key and metadata
pub fn complete_handshake(
    response: &HandshakeResponse,
    ephemeral_private_key: &str,
    expected_handshake_id: &str,
) -> Result<HandshakeResult> {
    // Verify response signature
    verify_handshake_response(response)?;

    // Verify handshake ID matches
    if response.handshake_id != expected_handshake_id {
        return Err(VaultlessError::Validation(
            "Handshake ID mismatch".to_string(),
        ));
    }

    // Check session not already expired
    if response.expires_at < Utc::now() {
        return Err(VaultlessError::Validation(
            "Session already expired".to_string(),
        ));
    }

    // Derive session key using ECDH + HKDF
    let session_key = exchange_and_derive(
        ephemeral_private_key,
        &response.ephemeral_exchange_pubkey,
        &response.session_id,
    )?;

    Ok(HandshakeResult {
        session_id: response.session_id.clone(),
        peer_signing_pubkey: response.signing_pubkey.clone(),
        session_key,
        expires_at: response.expires_at,
    })
}

/// Derive session key from response (responder side)
///
/// # Arguments
/// * `response` - The handshake response that was sent
/// * `ephemeral_private_key` - Responder's ephemeral X25519 private key
/// * `initiator_exchange_pubkey` - Initiator's ephemeral X25519 public key
///
/// # Returns
/// * HandshakeResult with session key
pub fn derive_responder_session_key(
    response: &HandshakeResponse,
    ephemeral_private_key: &str,
    initiator_exchange_pubkey: &str,
) -> Result<HandshakeResult> {
    // Derive session key using ECDH + HKDF
    let session_key = exchange_and_derive(
        ephemeral_private_key,
        initiator_exchange_pubkey,
        &response.session_id,
    )?;

    Ok(HandshakeResult {
        session_id: response.session_id.clone(),
        peer_signing_pubkey: String::new(), // Already known from request
        session_key,
        expires_at: response.expires_at,
    })
}

/// Verify handshake request signature
fn verify_handshake_request(request: &HandshakeRequest) -> Result<()> {
    let payload = format!(
        "{}||{}||{}||{}",
        request.handshake_id,
        request.signing_pubkey,
        request.ephemeral_exchange_pubkey,
        request.timestamp.to_rfc3339()
    );

    signing::verify_signature(payload.as_bytes(), &request.signature, &request.signing_pubkey)
}

/// Verify handshake response signature
fn verify_handshake_response(response: &HandshakeResponse) -> Result<()> {
    let payload = format!(
        "{}||{}||{}||{}||{}||{}",
        response.handshake_id,
        response.signing_pubkey,
        response.ephemeral_exchange_pubkey,
        response.timestamp.to_rfc3339(),
        response.session_id,
        response.expires_at.to_rfc3339()
    );

    signing::verify_signature(payload.as_bytes(), &response.signature, &response.signing_pubkey)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_handshake_flow() {
        // Alice (initiator) generates keypairs
        let alice_signing = keys::generate_signing_keypair().unwrap();
        let alice_ephemeral = keys::generate_exchange_keypair().unwrap();

        // Bob (responder) generates keypairs
        let bob_signing = keys::generate_signing_keypair().unwrap();
        let bob_ephemeral = keys::generate_exchange_keypair().unwrap();

        // Step 1: Alice creates handshake request
        let request = create_handshake_request(
            &alice_signing,
            &alice_ephemeral.public_key,
        )
        .unwrap();

        // Step 2: Bob responds to handshake
        let response = respond_to_handshake(
            &request,
            &bob_signing,
            &bob_ephemeral.public_key,
            60, // 60 minute session
        )
        .unwrap();

        // Step 3: Alice completes handshake
        let alice_result = complete_handshake(
            &response,
            &alice_ephemeral.private_key,
            &request.handshake_id,
        )
        .unwrap();

        // Step 4: Bob derives his session key
        let bob_result = derive_responder_session_key(
            &response,
            &bob_ephemeral.private_key,
            &request.ephemeral_exchange_pubkey,
        )
        .unwrap();

        // Both should have the same session key
        assert_eq!(alice_result.session_key, bob_result.session_key);
        assert_eq!(alice_result.session_id, bob_result.session_id);
        assert_eq!(alice_result.expires_at, bob_result.expires_at);

        // Verify they can encrypt/decrypt with the session key
        use crate::crypto::encryption::{encrypt_xchacha, decrypt_xchacha};

        let plaintext = b"Secret message via handshake!";
        let alice_key_vec = BASE64.decode(&alice_result.session_key).unwrap();
        let mut alice_key: [u8; 32] = alice_key_vec.try_into().unwrap();
        let encrypted = encrypt_xchacha(plaintext, &mut alice_key).unwrap();

        let bob_key_vec = BASE64.decode(&bob_result.session_key).unwrap();
        let mut bob_key: [u8; 32] = bob_key_vec.try_into().unwrap();
        let decrypted = decrypt_xchacha(&encrypted, &mut bob_key).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_handshake_request_signature_verification() {
        let signing = keys::generate_signing_keypair().unwrap();
        let exchange = keys::generate_exchange_keypair().unwrap();

        let request = create_handshake_request(&signing, &exchange.public_key).unwrap();

        // Should verify successfully
        assert!(verify_handshake_request(&request).is_ok());

        // Tampered request should fail
        let mut tampered = request.clone();
        tampered.ephemeral_exchange_pubkey = "tampered_key".to_string();
        assert!(verify_handshake_request(&tampered).is_err());
    }

    #[test]
    fn test_handshake_response_signature_verification() {
        let alice_signing = keys::generate_signing_keypair().unwrap();
        let alice_ephemeral = keys::generate_exchange_keypair().unwrap();
        let bob_signing = keys::generate_signing_keypair().unwrap();
        let bob_ephemeral = keys::generate_exchange_keypair().unwrap();

        let request = create_handshake_request(&alice_signing, &alice_ephemeral.public_key).unwrap();
        let response = respond_to_handshake(&request, &bob_signing, &bob_ephemeral.public_key, 60).unwrap();

        // Should verify successfully
        assert!(verify_handshake_response(&response).is_ok());

        // Tampered response should fail
        let mut tampered = response.clone();
        tampered.session_id = "tampered_session".to_string();
        assert!(verify_handshake_response(&tampered).is_err());
    }

    #[test]
    fn test_reject_expired_handshake_request() {
        let signing = keys::generate_signing_keypair().unwrap();
        let exchange = keys::generate_exchange_keypair().unwrap();

        let mut request = create_handshake_request(&signing, &exchange.public_key).unwrap();

        // Set timestamp to 10 minutes ago
        request.timestamp = Utc::now() - Duration::minutes(10);

        let bob_signing = keys::generate_signing_keypair().unwrap();
        let bob_exchange = keys::generate_exchange_keypair().unwrap();

        let result = respond_to_handshake(&request, &bob_signing, &bob_exchange.public_key, 60);
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_handshake_id_mismatch() {
        let alice_signing = keys::generate_signing_keypair().unwrap();
        let alice_ephemeral = keys::generate_exchange_keypair().unwrap();
        let bob_signing = keys::generate_signing_keypair().unwrap();
        let bob_ephemeral = keys::generate_exchange_keypair().unwrap();

        let request = create_handshake_request(&alice_signing, &alice_ephemeral.public_key).unwrap();
        let response = respond_to_handshake(&request, &bob_signing, &bob_ephemeral.public_key, 60).unwrap();

        // Try to complete with wrong handshake ID
        let result = complete_handshake(&response, &alice_ephemeral.private_key, "wrong-id");
        assert!(result.is_err());
    }
}
