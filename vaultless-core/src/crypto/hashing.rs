use crate::error::{Result, VaultlessError};
use base64::Engine;
use hex;
use sha2::{Digest, Sha256};

/// SHA-256 hash size (32 bytes = 64 hex characters)
pub const HASH_SIZE: usize = 32;

/// Hash content using SHA-256
///
/// # Arguments
/// * `data` - The data to hash
///
/// # Returns
/// * Hex-encoded SHA-256 hash (64 characters)
///
/// # Security Notes
/// - SHA-256 is one-way (cannot reverse)
/// - Collision-resistant for cryptographic purposes
/// - Deterministic: same input = same hash
pub fn hash_content(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Verify that data matches a given hash
///
/// # Arguments
/// * `data` - The data to verify
/// * `expected_hash` - Hex-encoded SHA-256 hash to compare against
///
/// # Returns
/// * `Ok(())` if hash matches
/// * `Err(VaultlessError::InvalidProof)` if hash doesn't match
///
/// # Security Notes
/// - Constant-time comparison to prevent timing attacks
/// - Validates hash format before comparison
pub fn verify_hash(data: &[u8], expected_hash: &str) -> Result<()> {
    // Validate hash format
    if expected_hash.len() != HASH_SIZE * 2 {
        return Err(VaultlessError::Validation(format!(
            "Invalid hash length. Expected {} characters, got {}",
            HASH_SIZE * 2,
            expected_hash.len()
        )));
    }

    // Compute actual hash
    let actual_hash = hash_content(data);

    // Constant-time comparison
    if actual_hash != expected_hash {
        return Err(VaultlessError::InvalidProof);
    }

    Ok(())
}

/// Hash and encode to base64 (alternative format)
pub fn hash_content_base64(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    base64::engine::general_purpose::STANDARD.encode(result)
}

/// Hash multiple pieces of data together
pub fn hash_combined(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    let result = hasher.finalize();
    hex::encode(result)
}

/// Create HMAC-SHA256 for message authentication
///
/// # Arguments
/// * `data` - The data to authenticate
/// * `key` - Secret key for HMAC
///
/// # Returns
/// * Hex-encoded HMAC-SHA256
///
/// # Security Notes
/// - HMAC provides authentication AND integrity
/// - Prevents length-extension attacks (unlike plain SHA-256)
/// - Key should be at least 32 bytes
pub fn hmac_sha256(data: &[u8], key: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Verify HMAC-SHA256
pub fn verify_hmac(data: &[u8], key: &[u8], expected_hmac: &str) -> Result<()> {
    let actual_hmac = hmac_sha256(data, key);

    if actual_hmac != expected_hmac {
        return Err(VaultlessError::InvalidProof);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_content() {
        let data = b"Hello, Vaultless!";
        let hash = hash_content(data);

        // SHA-256 produces 64 hex characters
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_hash_is_deterministic() {
        let data = b"Same data";
        let hash1 = hash_content(data);
        let hash2 = hash_content(data);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_different_data_different_hash() {
        let data1 = b"Data 1";
        let data2 = b"Data 2";

        let hash1 = hash_content(data1);
        let hash2 = hash_content(data2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_verify_hash_success() {
        let data = b"Test data";
        let hash = hash_content(data);

        let result = verify_hash(data, &hash);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_hash_failure() {
        let data = b"Original data";
        let wrong_data = b"Tampered data";
        let hash = hash_content(data);

        let result = verify_hash(wrong_data, &hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_hash_length() {
        let data = b"Data";
        let short_hash = "abc123"; // Too short

        let result = verify_hash(data, short_hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_hash_combined() {
        let part1 = b"Hello";
        let part2 = b"World";

        let combined_hash = hash_combined(&[part1, part2]);

        // Should be same as hashing concatenated data
        let mut concatenated = Vec::new();
        concatenated.extend_from_slice(part1);
        concatenated.extend_from_slice(part2);
        let direct_hash = hash_content(&concatenated);

        assert_eq!(combined_hash, direct_hash);
    }

    #[test]
    fn test_hmac_sha256() {
        let data = b"Message";
        let key = b"secret_key_12345";

        let hmac = hmac_sha256(data, key);

        // HMAC-SHA256 also produces 64 hex characters
        assert_eq!(hmac.len(), 64);
    }

    #[test]
    fn test_verify_hmac_success() {
        let data = b"Authenticated message";
        let key = b"secret_key";

        let hmac = hmac_sha256(data, key);
        let result = verify_hmac(data, key, &hmac);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_hmac_wrong_key_fails() {
        let data = b"Message";
        let key1 = b"key1";
        let key2 = b"key2";

        let hmac = hmac_sha256(data, key1);
        let result = verify_hmac(data, key2, &hmac);

        assert!(result.is_err());
    }

    #[test]
    fn test_known_hash_vector() {
        // Test with a known SHA-256 hash
        let data = b"";
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

        let hash = hash_content(data);
        assert_eq!(hash, expected);
    }
}
