
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::error::{Result, VaultlessError};

/// Standard nonce size for AES-GCM (96 bits / 12 bytes)
pub const NONCE_SIZE: usize = 12;

/// AES-256 key size (256 bits / 32 bytes)
pub const KEY_SIZE: usize = 32;

/// Encrypted data with nonce
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    /// Base64-encoded ciphertext
    pub ciphertext: String,
    /// Base64-encoded nonce (12 bytes)
    pub nonce: String,
}

/// Encrypt plaintext using AES-256-GCM
///
/// # Arguments
/// * `plaintext` - The data to encrypt
/// * `key` - Mutable slice for the encryption key (must be exactly 32 bytes; will be zeroized after use)
///
/// # Returns
/// * `Ok(EncryptedData)` on success
/// * `Err(VaultlessError::Encryption(_))` if key is wrong size or other crypto failure
///
/// # Security Notes
/// - Requires exactly 32 bytes for AES-256; shorter/longer keys fail early.
/// - Uses a randomly generated 12-byte nonce (GCM standard) for each encryption.
/// - Key is zeroized from memory after use.
/// - Never reuse the same nonce with the same key (nonce is returned for decryption).
pub fn encrypt(plaintext: &[u8], key: &mut [u8]) -> Result<EncryptedData> {
    if key.len() != KEY_SIZE {
        // KEY_SIZE = 32
        return Err(VaultlessError::Encryption(format!(
            "Invalid key size. Expected {} bytes for AES-256",
            KEY_SIZE
        )));
    }

    // Create cipher (validates length again internally, but we checked early for better errors)
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| VaultlessError::Encryption(format!("Failed to create cipher: {}", e)))?;

    // Generate random nonce (use 12 bytes for GCM; adjust NONCE_SIZE accordingly)
    let mut nonce_bytes = [0u8; NONCE_SIZE]; // NONCE_SIZE = 12
    getrandom::fill(&mut nonce_bytes)
        .map_err(|e| VaultlessError::Encryption(format!("Failed to generate nonce: {}", e)))?;

    let nonce: [u8; NONCE_SIZE] = nonce_bytes
    .as_slice()
    .try_into()
    .map_err(|_| VaultlessError::Encryption("Nonce length mismatch".to_string()))?;
let nonce = Nonce::from(nonce);


    // Encrypt
    let ciphertext_bytes = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| VaultlessError::Encryption(format!("Encryption failed: {}", e)))?;

    // Zeroize key from memory
    key.zeroize();

    // Encode to base64
    let ciphertext = BASE64.encode(&ciphertext_bytes);
    let nonce_b64 = BASE64.encode(nonce_bytes);

    Ok(EncryptedData {
        ciphertext,
        nonce: nonce_b64,
    })
}
/// Decrypt ciphertext using AES-256-GCM
///
/// # Arguments
/// * `encrypted` - The encrypted data with nonce
/// * `key` - 32-byte decryption key (will be zeroized after use)
///
/// # Returns
/// * Decrypted plaintext bytes
///
/// # Security Notes
/// - Verifies authentication tag automatically
/// - Key is zeroized from memory after use
/// - Returns error if authentication fails (tampered data)
pub fn decrypt(encrypted: &EncryptedData, key: &mut [u8; KEY_SIZE]) -> Result<Vec<u8>> {
    if key.len() != KEY_SIZE {
        return Err(VaultlessError::Decryption(
            "Invalid key size. Expected 32 bytes for AES-256".to_string(),
        ));
    }

    // Decode from base64
    let ciphertext_bytes = BASE64
        .decode(&encrypted.ciphertext)
        .map_err(|e| VaultlessError::Decryption(format!("Invalid base64 ciphertext: {}", e)))?;

    let nonce_bytes = BASE64
        .decode(&encrypted.nonce)
        .map_err(|e| VaultlessError::Decryption(format!("Invalid base64 nonce: {}", e)))?;

    if nonce_bytes.len() != NONCE_SIZE {
        return Err(VaultlessError::Decryption(format!(
            "Invalid nonce size. Expected {} bytes, got {}",
            NONCE_SIZE,
            nonce_bytes.len()
        )));
    }

    // Create cipher instance
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| VaultlessError::Decryption(format!("Failed to create cipher: {}", e)))?;

    let nonce: [u8; NONCE_SIZE] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VaultlessError::Decryption("Nonce length mismatch".to_string()))?;
    let nonce = Nonce::from(nonce);

    // Decrypt (automatically verifies authentication tag)
    let plaintext = cipher
        .decrypt(&nonce, ciphertext_bytes.as_ref())
        .map_err(|e| {
            VaultlessError::Decryption(format!("Decryption failed (possibly tampered data): {}", e))
        })?;

    // Zeroize key from memory
    key.zeroize();

    Ok(plaintext)
}

/// Encrypt plaintext and return result as strings (convenience function)
pub fn encrypt_to_strings(plaintext: &[u8], key: &mut [u8; KEY_SIZE]) -> Result<(String, String)> {
    let encrypted = encrypt(plaintext, key)?;
    Ok((encrypted.ciphertext, encrypted.nonce))
}

/// Decrypt from strings (convenience function)
pub fn decrypt_from_strings(
    ciphertext: &str,
    nonce: &str,
    key: &mut [u8; KEY_SIZE],
) -> Result<Vec<u8>> {
    let encrypted = EncryptedData {
        ciphertext: ciphertext.to_string(),
        nonce: nonce.to_string(),
    };
    decrypt(&encrypted, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let plaintext = b"Hello, Vaultless Data!";
        let mut key = [0u8; KEY_SIZE];
        getrandom::fill(&mut key).unwrap();

        // Make a copy since encrypt/decrypt zeroize the key
        let mut key_copy = key;

        let encrypted = encrypt(plaintext, &mut key).unwrap();
        let decrypted = decrypt(&encrypted, &mut key_copy).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_encrypt_produces_different_ciphertext() {
        let plaintext = b"Same plaintext";
        let mut key1 = [1u8; KEY_SIZE];
        let mut key2 = [1u8; KEY_SIZE];

        let encrypted1 = encrypt(plaintext, &mut key1).unwrap();
        let encrypted2 = encrypt(plaintext, &mut key2).unwrap();

        // Different nonces = different ciphertext
        assert_ne!(encrypted1.ciphertext, encrypted2.ciphertext);
        assert_ne!(encrypted1.nonce, encrypted2.nonce);
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let plaintext = b"Secret message";
        let mut key1 = [1u8; KEY_SIZE];
        let mut key2 = [2u8; KEY_SIZE];

        let encrypted = encrypt(plaintext, &mut key1).unwrap();
        let result = decrypt(&encrypted, &mut key2);

        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let plaintext = b"Authentic message";
        let mut key = [1u8; KEY_SIZE];
        let mut key_copy = key;

        let mut encrypted = encrypt(plaintext, &mut key).unwrap();

        // Tamper with ciphertext
        encrypted.ciphertext.push_str("tampered");

        let result = decrypt(&encrypted, &mut key_copy);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_key_size() {
        let plaintext = b"Test";
        let mut wrong_key = [0u8; 16]; // Wrong size

        let result = encrypt(plaintext, &mut wrong_key);
        assert!(result.is_err());
    }
}
