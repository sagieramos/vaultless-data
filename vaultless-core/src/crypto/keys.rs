use base64::prelude::*;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use getrandom;
use ring_compat::signature::ed25519::SigningKey;
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

use crate::crypto::{encryption, signing};
use crate::error::Result;

/// Ed25519 keypair (for signing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeypair {
    /// Base64-encoded private/signing key (32 bytes)
    pub private_key: String,
    /// Base64-encoded public/verifying key (32 bytes)
    pub public_key: String,
}

/// X25519 keypair (for key exchange)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeKeypair {
    /// Base64-encoded private/secret key (32 bytes)
    pub private_key: String,
    /// Base64-encoded public key (32 bytes)
    pub public_key: String,
}

/// Combined keypair for dual-key cryptography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualKeypair {
    /// Ed25519 keypair for signing/authentication
    pub signing: SigningKeypair,
    /// X25519 keypair for key exchange/encryption
    pub exchange: ExchangeKeypair,
}

/// Generate a new Ed25519 signing keypair
///
/// # Returns
/// * `SigningKeypair` with both keys base64-encoded
///
/// # Security Notes
/// - Uses cryptographically secure random number generator
/// - Private key should be stored securely (never in database)
/// - Public key can be shared freely
pub fn generate_signing_keypair() -> Result<SigningKeypair> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| {
        crate::error::VaultlessError::Internal(format!("Key generation failed: {}", e))
    })?;

    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();

    Ok(SigningKeypair {
        private_key: BASE64_STANDARD.encode(signing_key.to_bytes()),
        public_key: BASE64_STANDARD.encode(verifying_key.as_ref()),
    })
}

/// Generate a new X25519 key exchange keypair
///
/// # Returns
/// * `ExchangeKeypair` with both keys base64-encoded
///
/// # Security Notes
/// - Uses cryptographically secure random number generator
/// - Private key should be stored securely (never in database)
/// - Public key can be shared freely
/// - Used for ECDH key agreement to establish session keys
pub fn generate_exchange_keypair() -> Result<ExchangeKeypair> {
    let mut secret_bytes = [0u8; 32];
    getrandom::fill(&mut secret_bytes).map_err(|e| {
        crate::error::VaultlessError::Internal(format!("Key generation failed: {}", e))
    })?;

    let secret = X25519StaticSecret::from(secret_bytes);
    let public = X25519PublicKey::from(&secret);

    Ok(ExchangeKeypair {
        private_key: BASE64_STANDARD.encode(secret.to_bytes()),
        public_key: BASE64_STANDARD.encode(public.as_bytes()),
    })
}

/// Generate a complete dual-keypair (Ed25519 + X25519)
///
/// # Returns
/// * `DualKeypair` containing both signing and exchange keypairs
///
/// # Security Notes
/// - Generates two independent keypairs
/// - Ed25519 for signatures/authentication
/// - X25519 for key exchange/encryption
/// - Private keys should never be transmitted or stored in database
pub fn generate_dual_keypair() -> Result<DualKeypair> {
    Ok(DualKeypair {
        signing: generate_signing_keypair()?,
        exchange: generate_exchange_keypair()?,
    })
}

/// Generate a new AES-256 encryption key
///
/// # Returns
/// * Base64-encoded 32-byte key
///
/// # Security Notes
/// - Uses cryptographically secure random number generator
/// - Key should be stored securely (never in plaintext in database)
/// - Consider using key derivation for user passwords
pub fn generate_encryption_key() -> Result<String> {
    let mut key = [0u8; encryption::KEY_SIZE];
    getrandom::fill(&mut key).map_err(|e| {
        crate::error::VaultlessError::Internal(format!("Key generation failed: {}", e))
    })?;

    Ok(BASE64.encode(key))
}

/// Generates a secure random token of length 'n' bytes,
/// and returns it as a Base64 encoded array size.
pub fn generate_secure_token<const N: usize>() -> Result<[u8; N]> {
    let mut token_bytes = [0u8; N];

    getrandom::fill(&mut token_bytes).map_err(|e| {
        crate::error::VaultlessError::Internal(format!("Secure token generation failed: {}", e))
    })?;

    Ok(token_bytes)
}

/// Generate a secure API key with environment tag (e.g. "sk_live_..." or "sk_test_...")
///
/// # Arguments
/// * `prefix` - Static prefix like "sk" for secret keys, "pk" for publishable keys
/// * `env` - Environment tag ("live" or "test")
///
/// # Returns
/// * `String` - The generated API key in format: `{prefix}_{env}_{hex_encoded_32_bytes}`
///
/// # Format
/// * Uses hex encoding (64 characters) to match SQL-generated keys from create_application
/// * Example: `sk_live_a1b2c3d4e5f6789012345678901234567890123456789012345678901234`
///
/// # Example
/// ```
/// let secret_key = crypto::generate_api_key("sk", "live")?;
/// println!("Generated key: {}", secret_key);
/// // Output: sk_live_a1b2c3d4e5f6789012345678901234567890123456789012345678901234
/// ```
pub fn generate_api_key(prefix: &str, env: &str) -> Result<String> {
    let mut random_bytes = [0u8; 32];
    getrandom::fill(&mut random_bytes).map_err(|e| {
        crate::error::VaultlessError::Internal(format!("API key generation failed: {}", e))
    })?;
    // Use hex encoding to match SQL-generated keys from create_application function
    let encoded = hex::encode(random_bytes);
    let api_key = format!("{}_{}_{}", prefix, env, encoded);

    Ok(api_key)
}

/// Derive encryption key from password using PBKDF2
///
/// # Arguments
/// * `password` - User password
/// * `salt` - Unique salt (should be stored with user)
/// * `iterations` - Number of iterations (recommended: 100,000+)
///
/// # Returns
/// * Base64-encoded 32-byte derived key
///
/// # Security Notes
/// - PBKDF2 makes brute-force attacks expensive
/// - Salt must be unique per user (prevents rainbow tables)
/// - Higher iterations = more secure but slower
/// - Consider using Argon2 for even better security
pub fn derive_key_from_password(password: &str, salt: &[u8], iterations: u32) -> Result<String> {
    use ring::pbkdf2;
    use std::num::NonZeroU32;

    let iterations = NonZeroU32::new(iterations).ok_or_else(|| {
        crate::error::VaultlessError::Validation("Iterations must be > 0".to_string())
    })?;

    let mut derived_key = [0u8; encryption::KEY_SIZE];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        password.as_bytes(),
        &mut derived_key,
    );

    Ok(BASE64.encode(derived_key))
}

/// Generate a random salt for password-based key derivation
///
/// # Returns
/// * Base64-encoded 16-byte salt
pub fn generate_salt() -> Result<String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| {
        crate::error::VaultlessError::Internal(format!("Salt generation failed: {}", e))
    })?;

    Ok(BASE64.encode(salt))
}

/// Generate API key with prefix
///
/// # Arguments
/// * `prefix` - Key prefix (e.g., "vlt_")
///
/// # Returns
/// * Generated API key string and its SHA-256 hash
///
/// # Format
/// * `{prefix}{base64_random_32_bytes}`
/// * Example: `vlt_kX9mN2pQ...`
pub fn generate_api_key_hash(prefix: &str) -> Result<(String, String)> {
    let mut random_bytes = [0u8; 32];
    getrandom::fill(&mut random_bytes).map_err(|e| {
        crate::error::VaultlessError::Internal(format!("API key generation failed: {}", e))
    })?;

    // Create API key with prefix
    let key_suffix = BASE64.encode(random_bytes);
    let api_key = format!("{}{}", prefix, key_suffix);

    // Hash the key for storage
    let key_hash = crate::crypto::hash_content(api_key.as_bytes());

    Ok((api_key, key_hash))
}

/// Decode base64-encoded key to bytes
pub fn decode_key(encoded_key: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(encoded_key)
        .map_err(|e| crate::error::VaultlessError::Validation(format!("Invalid base64 key: {}", e)))
}

/// Decode base64-encoded key to fixed-size array (for AES keys)
pub fn decode_encryption_key(encoded_key: &str) -> Result<[u8; encryption::KEY_SIZE]> {
    let bytes = decode_key(encoded_key)?;

    if bytes.len() != encryption::KEY_SIZE {
        return Err(crate::error::VaultlessError::Validation(format!(
            "Invalid encryption key size. Expected {} bytes, got {}",
            encryption::KEY_SIZE,
            bytes.len()
        )));
    }

    let mut key = [0u8; encryption::KEY_SIZE];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Decode base64-encoded key to fixed-size array (for Ed25519 keys)
pub fn decode_signing_key(encoded_key: &str) -> Result<[u8; signing::PRIVATE_KEY_SIZE]> {
    let bytes = decode_key(encoded_key)?;

    if bytes.len() != signing::PRIVATE_KEY_SIZE {
        return Err(crate::error::VaultlessError::Validation(format!(
            "Invalid signing key size. Expected {} bytes, got {}",
            signing::PRIVATE_KEY_SIZE,
            bytes.len()
        )));
    }

    let mut key = [0u8; signing::PRIVATE_KEY_SIZE];
    key.copy_from_slice(&bytes);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_signing_keypair() {
        let keypair = generate_signing_keypair().unwrap();

        assert!(!keypair.private_key.is_empty());
        assert!(!keypair.public_key.is_empty());
        assert_ne!(keypair.private_key, keypair.public_key);
    }

    #[test]
    fn test_generate_encryption_key() {
        let key1 = generate_encryption_key().unwrap();
        let key2 = generate_encryption_key().unwrap();

        // Keys should be different each time
        assert_ne!(key1, key2);

        // Should be base64 encoded
        assert!(BASE64.decode(&key1).is_ok());
    }

    #[test]
    fn test_derive_key_from_password() {
        let password = "secure_password_123";
        let salt = b"unique_salt_12345";

        let key = derive_key_from_password(password, salt, 10_000).unwrap();

        // Derived key should be deterministic
        let key2 = derive_key_from_password(password, salt, 10_000).unwrap();
        assert_eq!(key, key2);

        // Different password = different key
        let key3 = derive_key_from_password("different_password", salt, 10_000).unwrap();
        assert_ne!(key, key3);
    }

    #[test]
    fn test_generate_salt() {
        let salt1 = generate_salt().unwrap();
        let salt2 = generate_salt().unwrap();

        // Salts should be different
        assert_ne!(salt1, salt2);
    }

    #[test]
    fn test_generate_api_key() {
        let (api_key, hash) = generate_api_key_hash("vlt_").unwrap();

        assert!(api_key.starts_with("vlt_"));
        assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars

        // Hash should match
        let computed_hash = crate::crypto::hash_content(api_key.as_bytes());
        assert_eq!(hash, computed_hash);
    }

    #[test]
    fn test_decode_encryption_key() {
        let encoded = generate_encryption_key().unwrap();
        let decoded = decode_encryption_key(&encoded).unwrap();

        assert_eq!(decoded.len(), encryption::KEY_SIZE);
    }

    #[test]
    fn test_decode_invalid_key_size() {
        let short_key = BASE64.encode([0u8; 16]); // Too short
        let result = decode_encryption_key(&short_key);

        assert!(result.is_err());
    }

    #[test]
    fn test_decode_invalid_base64() {
        let result = decode_key("not-valid-base64!@#$");
        assert!(result.is_err());
    }

    #[test]
    fn test_keypair_can_be_used_for_signing() {
        let keypair = generate_signing_keypair().unwrap();
        let private_bytes = decode_signing_key(&keypair.private_key).unwrap();

        let data = b"Test message";
        let signed = crate::crypto::sign_data(data, &private_bytes).unwrap();

        // Public key from signing should match keypair public key
        assert_eq!(signed.public_key, keypair.public_key);
    }

    #[test]
    fn test_generate_exchange_keypair() {
        let keypair = generate_exchange_keypair().unwrap();

        assert!(!keypair.private_key.is_empty());
        assert!(!keypair.public_key.is_empty());
        assert_ne!(keypair.private_key, keypair.public_key);

        // Should be valid base64
        assert!(BASE64.decode(&keypair.private_key).is_ok());
        assert!(BASE64.decode(&keypair.public_key).is_ok());

        // Keys should be 32 bytes when decoded
        let private_bytes = BASE64.decode(&keypair.private_key).unwrap();
        let public_bytes = BASE64.decode(&keypair.public_key).unwrap();
        assert_eq!(private_bytes.len(), 32);
        assert_eq!(public_bytes.len(), 32);
    }

    #[test]
    fn test_generate_dual_keypair() {
        let dual = generate_dual_keypair().unwrap();

        // Both keypairs should exist
        assert!(!dual.signing.private_key.is_empty());
        assert!(!dual.signing.public_key.is_empty());
        assert!(!dual.exchange.private_key.is_empty());
        assert!(!dual.exchange.public_key.is_empty());

        // All keys should be different
        assert_ne!(dual.signing.private_key, dual.exchange.private_key);
        assert_ne!(dual.signing.public_key, dual.exchange.public_key);
    }

    #[test]
    fn test_x25519_key_exchange() {
        // Generate two keypairs (Alice and Bob)
        let alice_keypair = generate_exchange_keypair().unwrap();
        let bob_keypair = generate_exchange_keypair().unwrap();

        // Decode keys
        let alice_private = BASE64.decode(&alice_keypair.private_key).unwrap();
        let alice_public_bytes = BASE64.decode(&alice_keypair.public_key).unwrap();
        let bob_private = BASE64.decode(&bob_keypair.private_key).unwrap();
        let bob_public_bytes = BASE64.decode(&bob_keypair.public_key).unwrap();

        // Perform ECDH
        let alice_secret = X25519StaticSecret::from(<[u8; 32]>::try_from(alice_private.as_slice()).unwrap());
        let bob_secret = X25519StaticSecret::from(<[u8; 32]>::try_from(bob_private.as_slice()).unwrap());
        let bob_public = X25519PublicKey::from(<[u8; 32]>::try_from(bob_public_bytes.as_slice()).unwrap());
        let alice_public = X25519PublicKey::from(<[u8; 32]>::try_from(alice_public_bytes.as_slice()).unwrap());

        let alice_shared = alice_secret.diffie_hellman(&bob_public);
        let bob_shared = bob_secret.diffie_hellman(&alice_public);

        // Both should derive the same shared secret
        assert_eq!(alice_shared.as_bytes(), bob_shared.as_bytes());
    }
}
