use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::Zeroize;

use crate::error::{Result, VaultlessError};

/// Session key size (32 bytes for XChaCha20-Poly1305)
pub const SESSION_KEY_SIZE: usize = 32;

/// Perform X25519 Diffie-Hellman key exchange
///
/// # Arguments
/// * `private_key` - Your X25519 private key (32 bytes, base64-encoded)
/// * `peer_public_key` - Peer's X25519 public key (32 bytes, base64-encoded)
///
/// # Returns
/// * 32-byte shared secret (NOT directly usable as encryption key - use derive_session_key)
///
/// # Security Notes
/// - The shared secret should be passed through HKDF before use
/// - Never use the raw shared secret as an encryption key
/// - Private key is zeroized after use
pub fn perform_key_exchange(private_key: &str, peer_public_key: &str) -> Result<[u8; 32]> {
    // Decode keys
    let private_bytes = BASE64
        .decode(private_key)
        .map_err(|e| VaultlessError::Validation(format!("Invalid base64 private key: {}", e)))?;

    let public_bytes = BASE64
        .decode(peer_public_key)
        .map_err(|e| VaultlessError::Validation(format!("Invalid base64 public key: {}", e)))?;

    if private_bytes.len() != 32 {
        return Err(VaultlessError::Validation(format!(
            "Invalid private key length: expected 32 bytes, got {}",
            private_bytes.len()
        )));
    }

    if public_bytes.len() != 32 {
        return Err(VaultlessError::Validation(format!(
            "Invalid public key length: expected 32 bytes, got {}",
            public_bytes.len()
        )));
    }

    // Convert to arrays
    let private_array: [u8; 32] = private_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VaultlessError::Internal("Failed to convert private key".to_string()))?;

    let public_array: [u8; 32] = public_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VaultlessError::Internal("Failed to convert public key".to_string()))?;

    // Perform ECDH
    let mut secret = X25519StaticSecret::from(private_array);
    let peer_public = X25519PublicKey::from(public_array);
    let shared = secret.diffie_hellman(&peer_public);

    // Zeroize the private key
    secret.zeroize();

    Ok(*shared.as_bytes())
}

/// Derive a session encryption key from shared secret using HKDF
///
/// # Arguments
/// * `shared_secret` - Raw shared secret from ECDH (will be zeroized)
/// * `info` - Context string (e.g., "vaultless-session-v1")
/// * `salt` - Optional salt (use session ID or nonce for uniqueness)
///
/// # Returns
/// * 32-byte session key suitable for XChaCha20-Poly1305
///
/// # Security Notes
/// - Uses HKDF-SHA256 for key derivation
/// - Info string should be unique to your protocol
/// - Salt adds additional entropy and session binding
/// - Input shared_secret is zeroized after derivation
pub fn derive_session_key(
    shared_secret: &mut [u8; 32],
    info: &[u8],
    salt: Option<&[u8]>,
) -> Result<[u8; SESSION_KEY_SIZE]> {
    let hkdf = match salt {
        Some(s) => Hkdf::<Sha256>::new(Some(s), shared_secret),
        None => Hkdf::<Sha256>::new(None, shared_secret),
    };

    let mut session_key = [0u8; SESSION_KEY_SIZE];
    hkdf.expand(info, &mut session_key)
        .map_err(|e| VaultlessError::Internal(format!("HKDF expansion failed: {}", e)))?;

    // Zeroize the shared secret
    shared_secret.zeroize();

    Ok(session_key)
}

/// Complete key exchange and derive session key (convenience function)
///
/// # Arguments
/// * `private_key` - Your X25519 private key (base64-encoded)
/// * `peer_public_key` - Peer's X25519 public key (base64-encoded)
/// * `session_id` - Unique session identifier (used as salt)
///
/// # Returns
/// * Base64-encoded 32-byte session key ready for XChaCha20-Poly1305
///
/// # Example
/// ```
/// let session_key = exchange_and_derive(
///     &my_private_key,
///     &peer_public_key,
///     "session-12345"
/// )?;
/// ```
pub fn exchange_and_derive(
    private_key: &str,
    peer_public_key: &str,
    session_id: &str,
) -> Result<String> {
    // Perform ECDH
    let mut shared_secret = perform_key_exchange(private_key, peer_public_key)?;

    // Derive session key using HKDF
    let info = b"vaultless-session-v1";
    let salt = session_id.as_bytes();
    let session_key = derive_session_key(&mut shared_secret, info, Some(salt))?;

    Ok(BASE64.encode(session_key))
}

/// Derive separate encryption and signing keys from shared secret
///
/// # Arguments
/// * `shared_secret` - Raw shared secret from ECDH
/// * `session_id` - Unique session identifier
///
/// # Returns
/// * (encryption_key, mac_key) - Both base64-encoded, 32 bytes each
///
/// # Use Case
/// - When you need separate keys for encryption and HMAC
/// - Provides additional security through key separation
pub fn derive_dual_keys(
    shared_secret: &mut [u8; 32],
    session_id: &str,
) -> Result<(String, String)> {
    let hkdf = Hkdf::<Sha256>::new(Some(session_id.as_bytes()), shared_secret);

    let mut encryption_key = [0u8; 32];
    let mut mac_key = [0u8; 32];

    // Derive encryption key
    hkdf.expand(b"vaultless-encryption-v1", &mut encryption_key)
        .map_err(|e| VaultlessError::Internal(format!("HKDF encryption key derivation failed: {}", e)))?;

    // Derive MAC key
    hkdf.expand(b"vaultless-mac-v1", &mut mac_key)
        .map_err(|e| VaultlessError::Internal(format!("HKDF MAC key derivation failed: {}", e)))?;

    // Zeroize the shared secret
    shared_secret.zeroize();

    Ok((BASE64.encode(encryption_key), BASE64.encode(mac_key)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::generate_exchange_keypair;

    #[test]
    fn test_perform_key_exchange() {
        // Generate two keypairs
        let alice = generate_exchange_keypair().unwrap();
        let bob = generate_exchange_keypair().unwrap();

        // Alice computes shared secret with Bob's public key
        let alice_shared = perform_key_exchange(&alice.private_key, &bob.public_key).unwrap();

        // Bob computes shared secret with Alice's public key
        let bob_shared = perform_key_exchange(&bob.private_key, &alice.public_key).unwrap();

        // Both should derive the same shared secret
        assert_eq!(alice_shared, bob_shared);
    }

    #[test]
    fn test_derive_session_key() {
        let mut shared_secret = [42u8; 32];
        let info = b"test-protocol";
        let salt = b"unique-session-123";

        let session_key1 = derive_session_key(&mut shared_secret.clone(), info, Some(salt)).unwrap();

        // Same inputs should produce same key
        let mut shared_secret2 = [42u8; 32];
        let session_key2 = derive_session_key(&mut shared_secret2, info, Some(salt)).unwrap();
        assert_eq!(session_key1, session_key2);

        // Different salt should produce different key
        let mut shared_secret3 = [42u8; 32];
        let session_key3 = derive_session_key(&mut shared_secret3, info, Some(b"different-salt")).unwrap();
        assert_ne!(session_key1, session_key3);
    }

    #[test]
    fn test_exchange_and_derive() {
        let alice = generate_exchange_keypair().unwrap();
        let bob = generate_exchange_keypair().unwrap();

        let session_id = "session-12345";

        // Alice and Bob derive same session key
        let alice_session_key = exchange_and_derive(&alice.private_key, &bob.public_key, session_id).unwrap();
        let bob_session_key = exchange_and_derive(&bob.private_key, &alice.public_key, session_id).unwrap();

        assert_eq!(alice_session_key, bob_session_key);

        // Different session ID = different key
        let alice_session_key2 = exchange_and_derive(&alice.private_key, &bob.public_key, "different-session").unwrap();
        assert_ne!(alice_session_key, alice_session_key2);
    }

    #[test]
    fn test_derive_dual_keys() {
        let mut shared_secret = [99u8; 32];
        let session_id = "session-xyz";

        let (enc_key, mac_key) = derive_dual_keys(&mut shared_secret, session_id).unwrap();

        // Keys should be different
        assert_ne!(enc_key, mac_key);

        // Both should be valid base64
        assert!(BASE64.decode(&enc_key).is_ok());
        assert!(BASE64.decode(&mac_key).is_ok());

        // Should be 32 bytes each
        assert_eq!(BASE64.decode(&enc_key).unwrap().len(), 32);
        assert_eq!(BASE64.decode(&mac_key).unwrap().len(), 32);
    }

    #[test]
    fn test_key_exchange_with_invalid_key_size() {
        let alice = generate_exchange_keypair().unwrap();
        let invalid_key = BASE64.encode([0u8; 16]); // Wrong size

        let result = perform_key_exchange(&alice.private_key, &invalid_key);
        assert!(result.is_err());
    }

    #[test]
    fn test_full_e2e_key_exchange_scenario() {
        // Simulate full handshake between Alice and Bob
        let alice_keypair = generate_exchange_keypair().unwrap();
        let bob_keypair = generate_exchange_keypair().unwrap();

        // Session metadata
        let session_id = "vaultless-session-abc123";

        // Both derive the same session key
        let alice_key = exchange_and_derive(
            &alice_keypair.private_key,
            &bob_keypair.public_key,
            session_id,
        ).unwrap();

        let bob_key = exchange_and_derive(
            &bob_keypair.private_key,
            &alice_keypair.public_key,
            session_id,
        ).unwrap();

        assert_eq!(alice_key, bob_key);

        // Verify it's a valid 32-byte key
        let key_bytes = BASE64.decode(&alice_key).unwrap();
        assert_eq!(key_bytes.len(), 32);

        // Can be used with XChaCha20-Poly1305
        use crate::crypto::encryption::{encrypt_xchacha, decrypt_xchacha};

        let plaintext = b"Secret message from Alice to Bob";
        let alice_key_vec = BASE64.decode(&alice_key).unwrap();
        let mut key_for_encrypt: [u8; 32] = alice_key_vec.try_into().unwrap();
        let encrypted = encrypt_xchacha(plaintext, &mut key_for_encrypt).unwrap();

        let bob_key_vec = BASE64.decode(&bob_key).unwrap();
        let mut key_for_decrypt: [u8; 32] = bob_key_vec.try_into().unwrap();
        let decrypted = decrypt_xchacha(&encrypted, &mut key_for_decrypt).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());
    }
}
