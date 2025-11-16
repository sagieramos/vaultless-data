// Add to Cargo.toml:
// webpki = "0.22"
// ring = "0.17"

use crate::error::{Result, VaultlessError};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use webpki::{EndEntityCert, Time, TrustAnchor};

// =============================================================================
// APPLE ROOT CA CERTIFICATE
// =============================================================================

/// Apple Root CA - G3 Root (DER format)
/// Download from: https://www.apple.com/certificateauthority/
/// This is the root CA used for App Attest certificates
///
/// To update:
/// 1. Download from Apple's website
/// 2. Convert to DER if needed: openssl x509 -in cert.pem -outform DER -out cert.der
/// 3. Include as bytes: include_bytes!("apple_root_ca_g3.der")
const APPLE_ROOT_CA_G3: &[u8] = include_bytes!("../../certs/AppleRootCA-G3.cer");

// Alternatively, hardcode the base64-encoded version (more portable):
// This is Apple Root CA - G3 Root certificate
const _APPLE_ROOT_CA_G3_BASE64: &str = "MIICQzCCAcmgAwIBAgIILcX8iNLFS5UwCgYIKoZIzj0EAwMwZzEbMBkGA1UEAwwSQXBwbGUgUm9vdCBDQSAtIEczMSYwJAYDVQQLDB1BcHBsZSBDZXJ0aWZpY2F0aW9uIEF1dGhvcml0eTETMBEGA1UECgwKQXBwbGUgSW5jLjELMAkGA1UEBhMCVVMwHhcNMTQwNDMwMTgxOTA2WhcNMzkwNDMwMTgxOTA2WjBnMRswGQYDVQQDDBJBcHBsZSBSb290IENBIC0gRzMxJjAkBgNVBAsMHUFwcGxlIENlcnRpZmljYXRpb24gQXV0aG9yaXR5MRMwEQYDVQQKDApBcHBsZSBJbmMuMQswCQYDVQQGEwJVUzB2MBAGByqGSM49AgEGBSuBBAAiA2IABJjpLz1AcqTtkyJygRMc3RCV8cWjTnHcFBbZDuWmBSp3ZHtfTjjTuxxEtX/1H7YyYl3J6YRbTzBPEVoA/VhYDKX1DyxNB0cTddqXl5dvMVztK517IDvYuVTZXpmkOlEKMaNCMEAwHQYDVR0OBBYEFLuw3qFYM4iapIqZ3r6966/ayySrMA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgEGMAoGCCqGSM49BAMDA2gAMGUCMQCD6cHEFl4aXTQY2e3v9GwOAEZLuN+yRhHFD/3meoyhpmvOwgPUnPWTxnS4at+qIxUCMG1mihDK1A3UT82NQz60imOlM27jbdoXt2QfyFMm+YhidDkLF1vLUagM6BgD56KyKA==";

// =============================================================================
// CERTIFICATE CHAIN VERIFICATION
// =============================================================================

/// Verify iOS App Attest certificate chain against Apple's root CA
pub fn verify_apple_certificate_chain(cert_chain: &[String]) -> Result<()> {
    if cert_chain.is_empty() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Certificate chain is empty".into(),
        ));
    }

    // 1. Load Apple Root CA
    let apple_root_der = BASE64
        .decode(APPLE_ROOT_CA_G3)
        .map_err(|e| VaultlessError::Internal(format!("Failed to decode Apple root CA: {}", e)))?;

    let trust_anchor = webpki::TrustAnchor::try_from_cert_der(&apple_root_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid Apple root CA: {:?}", e))
    })?;

    // 2. Decode leaf certificate (first in chain)
    let leaf_cert_der = BASE64.decode(&cert_chain[0]).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid leaf certificate: {}", e))
    })?;

    let leaf_cert = EndEntityCert::try_from(&leaf_cert_der[..]).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse leaf certificate: {:?}", e))
    })?;

    // 3. Decode intermediate certificates (rest of chain)
    let mut intermediates_der = Vec::new();
    for cert_b64 in &cert_chain[1..] {
        let cert_der = BASE64.decode(cert_b64).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid intermediate certificate: {}", e))
        })?;
        intermediates_der.push(cert_der);
    }

    // Convert to slices for webpki
    let intermediates: Vec<&[u8]> = intermediates_der.iter().map(|c| c.as_slice()).collect();

    // 4. Get current time for validity check
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| VaultlessError::Internal(format!("System time error: {}", e)))?;

    let time = Time::from_seconds_since_unix_epoch(now.as_secs());

    // 5. Verify the certificate chain
    leaf_cert
        .verify_is_valid_tls_server_cert(
            &[&webpki::ECDSA_P256_SHA256],
            &webpki::TlsServerTrustAnchors(&[trust_anchor]),
            &intermediates,
            time,
        )
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!(
                "Certificate chain verification failed: {:?}",
                e
            ))
        })?;

    Ok(())
}

// =============================================================================
// CERTIFICATE EXTENSION VERIFICATION
// =============================================================================

/// Extract and verify the App ID from the certificate
/// App ID = Team ID (10 chars) + "." + Bundle ID
pub fn verify_app_id_from_certificate(
    cert_der: &[u8],
    expected_team_id: &str,
    expected_bundle_id: &str,
) -> Result<()> {
    use x509_parser::prelude::*;

    // Parse certificate
    let (_, cert) = X509Certificate::from_der(cert_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse certificate: {}", e))
    })?;

    // Look for the App ID in certificate extensions
    // App Attest certificates have a custom extension with OID 1.2.840.113635.100.8.2
    const APP_ATTEST_EXTENSION_OID: &str = "1.2.840.113635.100.8.2";

    for ext in cert.extensions() {
        let oid_str = ext.oid.to_id_string();

        if oid_str == APP_ATTEST_EXTENSION_OID {
            // Extension value contains the App ID as an OCTET STRING
            // Format: <Team ID>.<Bundle ID>
            let app_id = String::from_utf8_lossy(ext.value);
            let expected_app_id = format!("{}.{}", expected_team_id, expected_bundle_id);

            if app_id.trim() != expected_app_id {
                return Err(VaultlessError::IntegrityCheckFailed(format!(
                    "App ID mismatch. Expected: {}, Found: {}",
                    expected_app_id, app_id
                )));
            }

            return Ok(());
        }
    }

    Err(VaultlessError::IntegrityCheckFailed(
        "App ID extension not found in certificate".into(),
    ))
}

// =============================================================================
// HELPER: DOWNLOAD APPLE ROOT CA
// =============================================================================

/// Helper function to download and save Apple Root CA (for setup)
/// This is NOT used at runtime - only for initial setup
#[cfg(test)]
pub async fn download_apple_root_ca() -> Result<Vec<u8>> {
    let url = "https://www.apple.com/certificateauthority/AppleRootCA-G3.cer";

    let response = reqwest::get(url).await.map_err(|e| {
        VaultlessError::Internal(format!("Failed to download Apple root CA: {}", e))
    })?;

    let cert_der = response
        .bytes()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Failed to read certificate: {}", e)))?
        .to_vec();

    // Save to file
    std::fs::write("apple_root_ca_g3.der", &cert_der)
        .map_err(|e| VaultlessError::Internal(format!("Failed to save certificate: {}", e)))?;

    println!("Apple Root CA saved to: apple_root_ca_g3.der");
    println!("Base64: {}", BASE64.encode(&cert_der));

    Ok(cert_der)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apple_root_ca_loads() {
        let apple_root_der = BASE64.decode(APPLE_ROOT_CA_G3).unwrap();
        let trust_anchor = webpki::TrustAnchor::try_from_cert_der(&apple_root_der);
        assert!(trust_anchor.is_ok());
    }

    #[test]
    fn test_empty_chain_fails() {
        let result = verify_apple_certificate_chain(&[]);
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore] // Only run manually
    async fn test_download_apple_root_ca() {
        let result = download_apple_root_ca().await;
        assert!(result.is_ok());
    }
}
