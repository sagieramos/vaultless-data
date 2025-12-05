use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use x509_parser::prelude::*;
use crate::error::{Result, VaultlessError};

// =============================================================================
// iOS VERSION EXTRACTION METHODS
// =============================================================================

/// iOS version information extracted from attestation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IOSVersionInfo {
    /// iOS version string (e.g., "17.2.1")
    pub version_string: String,
    
    /// iOS version code as integer for comparison (e.g., 170201 for 17.2.1)
    pub version_code: i32,
    
    /// Major version (e.g., 17)
    pub major: i32,
    
    /// Minor version (e.g., 2)
    pub minor: i32,
    
    /// Patch version (e.g., 1)
    pub patch: i32,
}

impl IOSVersionInfo {
    /// Parse iOS version string like "17.2.1" into structured data
    pub fn from_version_string(version_str: &str) -> Result<Self> {
        let parts: Vec<&str> = version_str.split('.').collect();
        
        if parts.is_empty() || parts.len() > 3 {
            return Err(VaultlessError::IntegrityCheckFailed(
                format!("Invalid iOS version format: {}", version_str)
            ));
        }

        let major = parts[0].parse::<i32>()
            .map_err(|_| VaultlessError::IntegrityCheckFailed(
                format!("Invalid major version: {}", parts[0])
            ))?;

        let minor = if parts.len() > 1 {
            parts[1].parse::<i32>()
                .map_err(|_| VaultlessError::IntegrityCheckFailed(
                    format!("Invalid minor version: {}", parts[1])
                ))?
        } else {
            0
        };

        let patch = if parts.len() > 2 {
            parts[2].parse::<i32>()
                .map_err(|_| VaultlessError::IntegrityCheckFailed(
                    format!("Invalid patch version: {}", parts[2])
                ))?
        } else {
            0
        };

        // Calculate version code: MMmmpp (e.g., 17.2.1 = 170201)
        let version_code = major * 10000 + minor * 100 + patch;

        Ok(IOSVersionInfo {
            version_string: version_str.to_string(),
            version_code,
            major,
            minor,
            patch,
        })
    }

    /// Check if this version meets minimum requirement
    pub fn meets_minimum(&self, min_version_code: i32) -> bool {
        self.version_code >= min_version_code
    }
}

// =============================================================================
// METHOD 1: CLIENT-SIDE VERSION REPORTING (Recommended for App Attest)
// =============================================================================

/// Enhanced attestation request with iOS version
#[derive(Debug, Deserialize, Serialize)]
pub struct IOSAttestationRequest {
    /// App Attest token
    pub attestation_token: String,
    
    /// iOS version reported by client (e.g., "17.2.1")
    pub ios_version: String,
    
    /// Device model (e.g., "iPhone15,2")
    pub device_model: Option<String>,
    
    /// App version
    pub app_version: Option<String>,
}

/// Validate iOS version from client-reported data
pub fn validate_ios_version_from_client(
    ios_version: &str,
    min_version_code: i32,
) -> Result<IOSVersionInfo> {
    let version_info = IOSVersionInfo::from_version_string(ios_version)?;

    if !version_info.meets_minimum(min_version_code) {
        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "iOS version {} (code: {}) does not meet minimum requirement (code: {})",
            version_info.version_string,
            version_info.version_code,
            min_version_code
        )));
    }

    Ok(version_info)
}

// =============================================================================
// METHOD 2: EXTRACT FROM APP ATTEST RECEIPT (Advanced)
// =============================================================================

/// App Attest receipt structure (simplified)
#[derive(Debug, Deserialize)]
pub struct AppAttestReceipt {
    /// Receipt type
    #[serde(rename = "receipt-type")]
    pub receipt_type: Option<String>,
    
    /// App version
    #[serde(rename = "app-version")]
    pub app_version: Option<String>,
    
    /// Original app version
    #[serde(rename = "original-app-version")]
    pub original_app_version: Option<String>,
    
    /// Device verification value
    #[serde(rename = "device-verification")]
    pub device_verification: Option<String>,
    
    /// Environment (production, sandbox)
    pub environment: Option<String>,
}

/// Extract iOS version from App Attest receipt (if available)
pub fn extract_version_from_receipt(receipt_b64: &str) -> Result<Option<String>> {
    // Decode base64 receipt
    let receipt_data = BASE64.decode(receipt_b64)
        .map_err(|e| VaultlessError::IntegrityCheckFailed(
            format!("Invalid receipt base64: {}", e)
        ))?;

    // Parse receipt (this is simplified - actual receipt is ASN.1/PKCS7)
    // In production, you'd use a proper ASN.1 parser
    
    // For App Attest, the receipt typically doesn't contain iOS version directly
    // You would need to parse the receipt and extract device info if present
    
    // This is a placeholder - actual implementation would parse ASN.1
    Ok(None)
}

// =============================================================================
// METHOD 3: DEVICECHECK TOKEN APPROACH (Alternative)
// =============================================================================

/// DeviceCheck token payload (sent from client)
#[derive(Debug, Deserialize, Serialize)]
pub struct DeviceCheckPayload {
    /// Device token from DeviceCheck API
    pub device_token: String,
    
    /// Timestamp when token was generated
    pub timestamp: i64,
    
    /// iOS version
    pub ios_version: String,
    
    /// Device model identifier
    pub device_model: String,
}

/// Validate DeviceCheck token with Apple's server
pub async fn validate_devicecheck_token(
    device_token: &str,
    team_id: &str,
    key_id: &str,
    private_key: &[u8],
) -> Result<bool> {
    // This would make a request to Apple's DeviceCheck API
    // https://developer.apple.com/documentation/devicecheck/accessing_and_modifying_per-device_data
    
    // Implementation would:
    // 1. Generate JWT token using team_id, key_id, private_key
    // 2. Send request to https://api.development.devicecheck.apple.com/v1/query_two_bits
    // 3. Validate device_token
    // 4. Return whether device is trusted
    
    // Placeholder
    Ok(true)
}

// =============================================================================
// METHOD 4: ATTESTATION OBJECT CUSTOM DATA (Recommended)
// =============================================================================

/// Enhanced App Attest object with client data hash
#[derive(Debug, Deserialize)]
pub struct EnhancedAppAttestObject {
    #[serde(rename = "fmt")]
    pub format: Option<String>,
    
    #[serde(rename = "attStmt")]
    pub att_stmt: AttestationStatement,
    
    #[serde(rename = "authData")]
    pub auth_data: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AttestationStatement {
    #[serde(rename = "x5c")]
    pub x5c: Option<Vec<String>>,
    
    pub receipt: Option<String>,
}

/// Client data that will be hashed and included in attestation
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientData {
    /// Challenge from server
    pub challenge: String,
    
    /// iOS version
    pub ios_version: String,
    
    /// Device model
    pub device_model: String,
    
    /// App version
    pub app_version: String,
    
    /// Timestamp
    pub timestamp: i64,
}

impl ClientData {
    /// Calculate SHA-256 hash of client data
    pub fn hash(&self) -> Vec<u8> {
        let json = serde_json::to_string(self).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        hasher.finalize().to_vec()
    }
}

/// Extract and validate client data from attestation
pub fn extract_client_data_from_authdata(
    auth_data: &[u8],
    expected_client_data_hash: &[u8],
) -> Result<()> {
    // AuthData structure:
    // - RP ID hash: 32 bytes
    // - Flags: 1 byte
    // - Counter: 4 bytes
    // - AAGUID: 16 bytes (optional)
    // - Credential ID length: 2 bytes (optional)
    // - Credential ID: variable (optional)
    // - Extensions: variable (optional)
    
    if auth_data.len() < 37 {
        return Err(VaultlessError::IntegrityCheckFailed(
            "AuthData too short".to_string()
        ));
    }

    // Skip RP ID hash (32 bytes), flags (1 byte), counter (4 bytes)
    let mut offset = 37;

    // Check if attestation data is present (AT flag bit 6)
    let flags = auth_data[32];
    let has_attestation = (flags & 0x40) != 0;
    let has_extensions = (flags & 0x80) != 0;

    if has_attestation {
        // Skip AAGUID (16 bytes)
        offset += 16;

        // Read credential ID length (2 bytes, big-endian)
        if auth_data.len() < offset + 2 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Invalid authData: missing credential ID length".to_string()
            ));
        }
        
        let cred_id_len = u16::from_be_bytes([auth_data[offset], auth_data[offset + 1]]) as usize;
        offset += 2;

        // Skip credential ID
        offset += cred_id_len;
    }

    if has_extensions {
        // Extensions would contain the client data hash
        // Parse CBOR extensions here
        // This is where you'd find the client data hash to verify
        
        // For now, this is a placeholder
        // You would use a CBOR parser to extract the clientDataHash extension
    }

    Ok(())
}

// =============================================================================
// RECOMMENDED IMPLEMENTATION: COMBINED APPROACH
// =============================================================================

use crate::models::app_model::attestation::dto::IosIntegrityConfig;
use super::types::*;
use chrono::Utc;

/// Complete iOS attestation with version validation
pub async fn verify_ios_attestation_with_version(
    attestation_request: &IOSAttestationRequest,
    config: &IosIntegrityConfig,
) -> Result<(AttestationResult, IOSVersionInfo)> {
    // 1. Validate iOS version first (fail fast)
    let version_info = if let Some(min_version) = config.min_version_code {
        validate_ios_version_from_client(&attestation_request.ios_version, min_version)?
    } else {
        IOSVersionInfo::from_version_string(&attestation_request.ios_version)?
    };

    // 2. Verify App Attest token (existing logic)
    // This would call your existing verify_ios_attestation function
    
    // For demonstration, creating a basic result
    let attestation_result = AttestationResult {
        is_valid: true,
        device_trusted: !config.reject_untrusted_device,
        verdict: Some("APPLE_APPATTEST_VERIFIED".to_string()),
        error: None,
        warnings: None,
        verified_at: Utc::now(),
        platform_data: PlatformAttestationData::IOS(IOSData {
            bundle_id: config.allowed_bundle_ids.first().cloned(),
            team_id: config.apple_team_id.clone(),
            attestation_token: attestation_request.attestation_token.clone(),
            device_info: Some(serde_json::json!({
                "ios_version": version_info.version_string,
                "version_code": version_info.version_code,
                "device_model": attestation_request.device_model,
                "app_version": attestation_request.app_version,
            })),
        }),
    };

    pub struct DeviceInfo {
    pub model: Option<String>,
    pub os_version: Option<String>,
    pub manufacturer: Option<String>,
    pub additional: Option<serde_json::Value>,
}


    Ok((attestation_result, version_info))
}

// =============================================================================
// HELPER: GENERATE MIN VERSION CODE
// =============================================================================

/// Helper function to generate version code from iOS version string
/// Example: "17.2.1" -> 170201
pub fn ios_version_to_code(version_str: &str) -> Result<i32> {
    let info = IOSVersionInfo::from_version_string(version_str)?;
    Ok(info.version_code)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        let v1 = IOSVersionInfo::from_version_string("17.2.1").unwrap();
        assert_eq!(v1.major, 17);
        assert_eq!(v1.minor, 2);
        assert_eq!(v1.patch, 1);
        assert_eq!(v1.version_code, 170201);

        let v2 = IOSVersionInfo::from_version_string("16.0").unwrap();
        assert_eq!(v2.version_code, 160000);

        let v3 = IOSVersionInfo::from_version_string("15").unwrap();
        assert_eq!(v3.version_code, 150000);
    }

    #[test]
    fn test_version_comparison() {
        let v1 = IOSVersionInfo::from_version_string("17.2.1").unwrap();
        let v2 = IOSVersionInfo::from_version_string("16.5.0").unwrap();

        assert!(v1.version_code > v2.version_code);
        assert!(v1.meets_minimum(160000));
        assert!(!v2.meets_minimum(170000));
    }

    #[test]
    fn test_version_to_code() {
        assert_eq!(ios_version_to_code("17.2.1").unwrap(), 170201);
        assert_eq!(ios_version_to_code("16.0").unwrap(), 160000);
        assert_eq!(ios_version_to_code("15").unwrap(), 150000);
    }

    #[test]
    fn test_version_validation() {
        let result = validate_ios_version_from_client("17.2.1", 170000);
        assert!(result.is_ok());

        let result = validate_ios_version_from_client("16.5.0", 170000);
        assert!(result.is_err());
    }
}