use base64::prelude::*;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use getrandom;
use ring_compat::signature::ed25519::SigningKey;
use serde::{Deserialize, Serialize};

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
pub fn generate_api_key(prefix: &str) -> Result<(String, String)> {
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
        let (api_key, hash) = generate_api_key("vlt_").unwrap();

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
}
