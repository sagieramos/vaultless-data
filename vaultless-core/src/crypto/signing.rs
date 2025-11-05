use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use ed25519::Signature;
use ring_compat::signature::ed25519::{SigningKey, VerifyingKey};
use ring_compat::signature::{Signer, Verifier};
use serde::{Deserialize, Serialize};

use crate::error::{Result, VaultlessError};

/// Ed25519 public key size (32 bytes)
pub const PUBLIC_KEY_SIZE: usize = 32;

/// Ed25519 private key size (32 bytes)
pub const PRIVATE_KEY_SIZE: usize = 32;

/// Ed25519 signature size (64 bytes)
pub const SIGNATURE_SIZE: usize = 64;

/// Signed data with signature and public key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedData {
    /// Base64-encoded Ed25519 signature (64 bytes)
    pub signature: String,
    /// Base64-encoded Ed25519 public key (32 bytes)
    pub public_key: String,
    /// Original data that was signed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<u8>>,
}

/// Sign data using Ed25519
///
/// # Arguments
/// * `data` - The data to sign
/// * `private_key` - 32-byte Ed25519 private/signing key
///
/// # Returns
/// * `SignedData` containing signature and corresponding public key
///
/// # Security Notes
/// - Uses deterministic Ed25519 (RFC 8032)
/// - Same data + key = same signature (for verification)
/// - Public key is derived from private key
pub fn sign_data(data: &[u8], private_key: &[u8; PRIVATE_KEY_SIZE]) -> Result<SignedData> {
    // Create signing key
    let signing_key = SigningKey::from_bytes(private_key);

    // Sign the data
    let signature = signing_key
        .try_sign(data)
        .map_err(|e| VaultlessError::Validation(format!("Signing failed: {}", e)))?;

    // Get public key
    let verifying_key = signing_key.verifying_key();

    Ok(SignedData {
        signature: BASE64.encode(signature.to_bytes()),
        public_key: BASE64.encode(verifying_key.as_ref()),
        data: None, // Don't include data by default
    })
}

/// Verify signature using Ed25519
///
/// # Arguments
/// * `data` - The original data that was signed
/// * `signature` - Base64-encoded signature
/// * `public_key` - Base64-encoded public key
///
/// # Returns
/// * `Ok(())` if signature is valid
/// * `Err(VaultlessError::SignatureVerificationFailed)` if invalid
///
/// # Security Notes
/// - Constant-time verification
/// - Checks both signature and public key validity
/// - Returns error on any malformation
pub fn verify_signature(data: &[u8], signature: &str, public_key: &str) -> Result<()> {
    // Decode signature
    let signature_bytes = BASE64
        .decode(signature)
        .map_err(|e| VaultlessError::Validation(format!("Invalid base64 signature: {}", e)))?;

    if signature_bytes.len() != SIGNATURE_SIZE {
        return Err(VaultlessError::Validation(format!(
            "Invalid signature size. Expected {} bytes, got {}",
            SIGNATURE_SIZE,
            signature_bytes.len()
        )));
    }

    // Decode public key
    let public_key_bytes = BASE64
        .decode(public_key)
        .map_err(|e| VaultlessError::Validation(format!("Invalid base64 public key: {}", e)))?;

    if public_key_bytes.len() != PUBLIC_KEY_SIZE {
        return Err(VaultlessError::Validation(format!(
            "Invalid public key size. Expected {} bytes, got {}",
            PUBLIC_KEY_SIZE,
            public_key_bytes.len()
        )));
    }

    // Create signature array
    let sig_array: [u8; SIGNATURE_SIZE] = signature_bytes
        .try_into()
        .map_err(|_| VaultlessError::Validation("Signature length mismatch".to_string()))?;
    let signature = Signature::from_bytes(&sig_array);

    // Create public key array
    let pub_array: [u8; PUBLIC_KEY_SIZE] = public_key_bytes
        .try_into()
        .map_err(|_| VaultlessError::Validation("Public key length mismatch".to_string()))?;

    // Create verifying key
    let verifying_key = VerifyingKey::from_slice(&pub_array)
        .map_err(|e| VaultlessError::Validation(format!("Invalid public key: {}", e)))?;

    // Verify
    verifying_key
        .verify(data, &signature)
        .map_err(|_| VaultlessError::SignatureVerificationFailed)?;

    Ok(())
}

/// Sign data and include the data in the response
pub fn sign_with_data(data: &[u8], private_key: &[u8; PRIVATE_KEY_SIZE]) -> Result<SignedData> {
    let mut signed = sign_data(data, private_key)?;
    signed.data = Some(data.to_vec());
    Ok(signed)
}

/// Verify SignedData struct (convenience function)
pub fn verify_signed_data(signed: &SignedData, expected_data: &[u8]) -> Result<()> {
    verify_signature(expected_data, &signed.signature, &signed.public_key)
}

/// Extract public key from private key
pub fn get_public_key(private_key: &[u8; PRIVATE_KEY_SIZE]) -> Result<String> {
    let signing_key = SigningKey::from_bytes(private_key);
    let verifying_key = signing_key.verifying_key();
    Ok(BASE64.encode(verifying_key.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() {
        let data = b"Important message";
        let private_key = [1u8; PRIVATE_KEY_SIZE];

        let signed = sign_data(data, &private_key).unwrap();
        let result = verify_signature(data, &signed.signature, &signed.public_key);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_with_wrong_data_fails() {
        let data = b"Original message";
        let wrong_data = b"Tampered message";
        let private_key = [1u8; PRIVATE_KEY_SIZE];

        let signed = sign_data(data, &private_key).unwrap();
        let result = verify_signature(wrong_data, &signed.signature, &signed.public_key);

        assert!(result.is_err());
    }

    #[test]
    fn test_verify_with_wrong_public_key_fails() {
        let data = b"Message";
        let private_key1 = [1u8; PRIVATE_KEY_SIZE];
        let private_key2 = [2u8; PRIVATE_KEY_SIZE];

        let signed1 = sign_data(data, &private_key1).unwrap();
        let signed2 = sign_data(data, &private_key2).unwrap();

        // Try to verify signed1 with signed2's public key
        let result = verify_signature(data, &signed1.signature, &signed2.public_key);
        assert!(result.is_err());
    }

    #[test]
    fn test_deterministic_signatures() {
        let data = b"Same data";
        let private_key = [42u8; PRIVATE_KEY_SIZE];

        let signed1 = sign_data(data, &private_key).unwrap();
        let signed2 = sign_data(data, &private_key).unwrap();

        // Ed25519 is deterministic: same input = same signature
        assert_eq!(signed1.signature, signed2.signature);
        assert_eq!(signed1.public_key, signed2.public_key);
    }

    #[test]
    fn test_get_public_key() {
        let private_key = [99u8; PRIVATE_KEY_SIZE];
        let public_key = get_public_key(&private_key).unwrap();

        // Sign with private key
        let data = b"Test";
        let signed = sign_data(data, &private_key).unwrap();

        // Public key should match
        assert_eq!(public_key, signed.public_key);
    }

    #[test]
    fn test_invalid_signature_format() {
        let data = b"Data";
        let result = verify_signature(data, "invalid-base64!@#", "some-key");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_signature_length() {
        let data = b"Data";
        let short_sig = BASE64.encode([0u8; 32]); // Too short
        let valid_key = BASE64.encode([0u8; PUBLIC_KEY_SIZE]);

        let result = verify_signature(data, &short_sig, &valid_key);
        assert!(result.is_err());
    }
}
