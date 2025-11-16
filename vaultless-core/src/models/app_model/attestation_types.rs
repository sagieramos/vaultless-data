use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use validator::Validate;

use crate::error::{Result, VaultlessError};

// =============================================================================
// Platform Enum
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[serde(rename = "ios")]
    IOS,
    #[serde(rename = "android")]
    Android,
    #[serde(rename = "web")]
    Web,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::IOS => "ios",
            Platform::Android => "android",
            Platform::Web => "web",
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "ios" => Ok(Platform::IOS),
            "android" => Ok(Platform::Android),
            "web" => Ok(Platform::Web),
            _ => Err(VaultlessError::Validation(format!(
                "Invalid platform: {}",
                s
            ))),
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// Attestation Metadata (stored in Client.metadata JSONB)
// =============================================================================

/// Complete attestation information stored in the client's metadata JSONB.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttestationMetadata {
    /// Platform: ios, android, or web
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,

    /// Bundle ID (iOS) or Package Name (Android)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,

    /// Unique device identifier (IDFV, Android ID, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,

    /// Application version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,

    /// Detailed attestation information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<AttestationDetails>,

    /// Device information (optional, for analytics/debugging)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_info: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationDetails {
    /// SHA-256 hash of the verified certificate
    pub certificate_hash: String,

    /// Whether the device passed integrity checks
    pub device_trusted: bool,

    /// When attestation was last verified
    pub verified_at: DateTime<Utc>,

    /// Attestation verdict/status from platform
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,

    /// Last challenge used (for audit trail)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_challenge: Option<String>,

    /// Number of times successfully attested
    #[serde(default)]
    pub attestation_count: u32,

    /// Any warnings or notes from attestation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device model (e.g., "iPhone 15 Pro", "Pixel 8")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// OS version (e.g., "17.0", "14")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,

    /// Device manufacturer (Android)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,

    /// Additional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional: Option<serde_json::Value>,
}

// =============================================================================
// Request/Response Types
// =============================================================================

/// Request to verify platform attestation during registration or re-attestation
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AttestationRequest {
    /// Platform being attested
    pub platform: Platform,

    /// Bundle ID or package name
    #[validate(length(min = 1, max = 255))]
    pub bundle_id: String,

    /// Device identifier
    #[validate(length(min = 1, max = 255))]
    pub device_id: String,

    /// The attestation token from the platform (App Attest or Play Integrity)
    #[validate(length(min = 32, max = 8192))]
    pub attestation_token: String,

    /// Optional: challenge/nonce that was signed (for replay protection)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 8, max = 128))]
    pub nonce: Option<String>,

    /// Application version
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50))]
    pub app_version: Option<String>,

    /// Optional device information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_info: Option<DeviceInfo>,
}

/// Result of attestation verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationResult {
    /// Whether attestation passed all checks
    pub is_valid: bool,

    /// Verified certificate hash
    pub certificate_hash: String,

    /// Verified bundle ID
    pub bundle_id: String,

    /// Platform that was attested
    pub platform: Platform,

    /// Whether device is trusted
    pub device_trusted: bool,

    /// Platform-specific verdict (e.g., "PLAY_RECOGNIZED", "MEETS_DEVICE_INTEGRITY")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,

    /// Error message if validation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Warnings (non-fatal issues)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,

    /// When this attestation was verified
    pub verified_at: DateTime<Utc>,
}

// =============================================================================
// Helper Methods for Client Metadata
// =============================================================================

impl AttestationMetadata {
    /// Create new attestation metadata from a successful attestation result
    pub fn from_result(
        result: AttestationResult,
        device_id: String,
        app_version: Option<String>,
        device_info: Option<DeviceInfo>,
    ) -> Self {
        Self {
            platform: Some(result.platform),
            bundle_id: Some(result.bundle_id),
            device_id: Some(device_id),
            app_version,
            attestation: Some(AttestationDetails {
                certificate_hash: result.certificate_hash,
                device_trusted: result.device_trusted,
                verified_at: result.verified_at,
                verdict: result.verdict,
                last_challenge: None,
                attestation_count: 1,
                warnings: result.warnings,
            }),
            device_info,
        }
    }

    /// Update existing attestation metadata with new verification
    pub fn update_from_result(&mut self, result: AttestationResult) {
        if let Some(ref mut attestation) = self.attestation {
            attestation.certificate_hash = result.certificate_hash;
            attestation.device_trusted = result.device_trusted;
            attestation.verified_at = result.verified_at;
            attestation.verdict = result.verdict;
            attestation.attestation_count += 1;
            attestation.warnings = result.warnings;
        } else {
            self.attestation = Some(AttestationDetails {
                certificate_hash: result.certificate_hash,
                device_trusted: result.device_trusted,
                verified_at: result.verified_at,
                verdict: result.verdict,
                last_challenge: None,
                attestation_count: 1,
                warnings: result.warnings,
            });
        }
        self.platform = Some(result.platform);
        self.bundle_id = Some(result.bundle_id);
    }

    /// Check if attestation needs refresh (e.g., older than 30 days)
    pub fn needs_reattesation(&self, days: i64) -> bool {
        match &self.attestation {
            Some(att) => {
                let age = Utc::now().signed_duration_since(att.verified_at).num_days();
                age > days
            }
            None => true,
        }
    }

    /// Get the last verified timestamp
    pub fn last_verified_at(&self) -> Option<DateTime<Utc>> {
        self.attestation.as_ref().map(|a| a.verified_at)
    }

    /// Check if device is trusted
    pub fn is_device_trusted(&self) -> bool {
        self.attestation
            .as_ref()
            .map(|a| a.device_trusted)
            .unwrap_or(false)
    }

    /// Get certificate hash if available
    pub fn certificate_hash(&self) -> Option<&str> {
        self.attestation
            .as_ref()
            .map(|a| a.certificate_hash.as_str())
    }

    /// Merge attestation metadata into existing client metadata JSONB
    pub fn merge_into_metadata(
        &self,
        existing: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let mut metadata = existing.unwrap_or(serde_json::json!({}));

        if let Some(obj) = metadata.as_object_mut() {
            // Serialize attestation metadata
            let attestation_value = serde_json::to_value(self)
                .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

            // Merge platform-specific fields at root level
            if let Some(platform) = &self.platform {
                obj.insert("platform".to_string(), serde_json::json!(platform));
            }
            if let Some(bundle_id) = &self.bundle_id {
                obj.insert("bundle_id".to_string(), serde_json::json!(bundle_id));
            }
            if let Some(device_id) = &self.device_id {
                obj.insert("device_id".to_string(), serde_json::json!(device_id));
            }
            if let Some(app_version) = &self.app_version {
                obj.insert("app_version".to_string(), serde_json::json!(app_version));
            }

            // Store full attestation details under "attestation" key
            obj.insert("attestation".to_string(), attestation_value);
        }

        Ok(metadata)
    }

    /// Extract attestation metadata from client metadata JSONB
    pub fn from_metadata(metadata: Option<&serde_json::Value>) -> Result<Option<Self>> {
        let Some(metadata) = metadata else {
            return Ok(None);
        };

        // Try to parse the full structure from the "attestation" key
        if let Some(attestation_value) = metadata.get("attestation") {
            let attestation_meta: AttestationMetadata =
                serde_json::from_value(attestation_value.clone()).map_err(|e| {
                    VaultlessError::Serialization(format!(
                        "Failed to parse attestation metadata: {}",
                        e
                    ))
                })?;
            return Ok(Some(attestation_meta));
        }

        // Fallback: Try to construct from root-level fields
        let platform = metadata
            .get("platform")
            .and_then(|v| v.as_str())
            .and_then(|s| Platform::from_str(s).ok());

        let bundle_id = metadata
            .get("bundle_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        let device_id = metadata
            .get("device_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        let app_version = metadata
            .get("app_version")
            .and_then(|v| v.as_str())
            .map(String::from);

        // If we have at least platform or bundle_id, return partial metadata
        if platform.is_some() || bundle_id.is_some() {
            Ok(Some(AttestationMetadata {
                platform,
                bundle_id,
                device_id,
                app_version,
                attestation: None,
                device_info: None,
            }))
        } else {
            Ok(None)
        }
    }
}

// =============================================================================
// Configuration Helpers (from Application.integrity_config)
// =============================================================================

/// Extract expected certificate hash from integrity config (Android only)
pub fn get_expected_cert_hash(
    integrity_config: &serde_json::Value,
    platform: Platform,
) -> Option<String> {
    match platform {
        Platform::Android => integrity_config
            .get("android")
            .and_then(|p| p.get("allowed_certificate_sha256"))
            .and_then(|v| v.as_str())
            .map(String::from),
        Platform::IOS | Platform::Web => None,
    }
}

/// Extract Apple Team ID from integrity config (iOS only)
pub fn get_apple_team_id(integrity_config: &serde_json::Value) -> Option<String> {
    integrity_config
        .get("ios")
        .and_then(|p| p.get("apple_team_id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Extract Google Cloud credentials from integrity config (Android only)
pub fn get_google_credentials(integrity_config: &serde_json::Value) -> Option<(String, String)> {
    let android = integrity_config.get("android")?;

    let project = android
        .get("google_cloud_project")
        .and_then(|v| v.as_str())
        .map(String::from)?;

    let api_key = android
        .get("google_api_key")
        .and_then(|v| v.as_str())
        .map(String::from)?;

    Some((project, api_key))
}

/// Extract allowed bundle IDs from integrity config
pub fn get_allowed_bundle_ids(
    integrity_config: &serde_json::Value,
    platform: Platform,
) -> Option<Vec<String>> {
    integrity_config
        .get(platform.as_str())
        .and_then(|p| p.get("allowed_bundle_ids"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
}

/// Extract minimum version code from integrity config
pub fn get_min_version_code(
    integrity_config: &serde_json::Value,
    platform: Platform,
) -> Option<i32> {
    integrity_config
        .get(platform.as_str())
        .and_then(|p| p.get("min_version_code"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
}

/// Check if untrusted devices should be rejected
pub fn should_reject_untrusted_device(
    integrity_config: &serde_json::Value,
    platform: Platform,
) -> bool {
    integrity_config
        .get(platform.as_str())
        .and_then(|p| p.get("reject_untrusted_device"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Validate attestation request against application integrity config
pub fn validate_attestation_config(
    request: &AttestationRequest,
    integrity_config: &serde_json::Value,
) -> Result<Option<String>> {
    match request.platform {
        Platform::IOS => {
            // iOS uses Apple Team ID instead of certificate hash
            let _team_id = get_apple_team_id(integrity_config).ok_or_else(|| {
                VaultlessError::IntegrityCheckFailed(
                    "No Apple Team ID configured for iOS attestation".into(),
                )
            })?;

            // Check bundle IDs if configured
            if let Some(allowed_bundles) =
                get_allowed_bundle_ids(integrity_config, request.platform)
            {
                if !allowed_bundles.is_empty() && !allowed_bundles.contains(&request.bundle_id) {
                    return Err(VaultlessError::IntegrityCheckFailed(format!(
                        "Bundle ID '{}' not in allowed list",
                        request.bundle_id
                    )));
                }
            }

            // Check minimum version if configured
            if let Some(min_version) = get_min_version_code(integrity_config, request.platform) {
                if let Some(version_str) = &request.app_version {
                    if let Ok(version_code) = version_str.parse::<i32>() {
                        if version_code < min_version {
                            return Err(VaultlessError::IntegrityCheckFailed(format!(
                                "App version {} is below minimum required version {}",
                                version_code, min_version
                            )));
                        }
                    }
                }
            }

            Ok(None) // iOS doesn't return cert hash
        }
        Platform::Android => {
            // Android uses certificate hash
            let cert_hash =
                get_expected_cert_hash(integrity_config, request.platform).ok_or_else(|| {
                    VaultlessError::IntegrityCheckFailed(
                        "No certificate hash configured for Android attestation".into(),
                    )
                })?;

            // Check bundle IDs if configured
            if let Some(allowed_bundles) =
                get_allowed_bundle_ids(integrity_config, request.platform)
            {
                if !allowed_bundles.is_empty() && !allowed_bundles.contains(&request.bundle_id) {
                    return Err(VaultlessError::IntegrityCheckFailed(format!(
                        "Bundle ID '{}' not in allowed list",
                        request.bundle_id
                    )));
                }
            }

            // Check minimum version
            if let Some(min_version) = get_min_version_code(integrity_config, request.platform) {
                if let Some(version_str) = &request.app_version {
                    if let Ok(version_code) = version_str.parse::<i32>() {
                        if version_code < min_version {
                            return Err(VaultlessError::IntegrityCheckFailed(format!(
                                "App version {} is below minimum required version {}",
                                version_code, min_version
                            )));
                        }
                    }
                }
            }

            Ok(Some(cert_hash))
        }
        Platform::Web => Err(VaultlessError::Validation(
            "Web platform does not support mobile attestation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_serialization() {
        let platform = Platform::IOS;
        let json = serde_json::to_string(&platform).unwrap();
        assert_eq!(json, r#""ios""#);

        let parsed: Platform = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Platform::IOS);
    }

    #[test]
    fn test_attestation_metadata_merge() {
        let attestation_meta = AttestationMetadata {
            platform: Some(Platform::IOS),
            bundle_id: Some("com.example.app".to_string()),
            device_id: Some("device-123".to_string()),
            app_version: Some("1.0.0".to_string()),
            attestation: Some(AttestationDetails {
                certificate_hash: "abc123".to_string(),
                device_trusted: true,
                verified_at: Utc::now(),
                verdict: Some("valid".to_string()),
                last_challenge: None,
                attestation_count: 1,
                warnings: None,
            }),
            device_info: None,
        };

        let existing = serde_json::json!({
            "custom_field": "value"
        });

        let merged = attestation_meta
            .merge_into_metadata(Some(existing))
            .unwrap();

        assert!(merged.get("platform").is_some());
        assert!(merged.get("bundle_id").is_some());
        assert!(merged.get("attestation").is_some());
        assert_eq!(merged.get("custom_field").unwrap(), "value");
    }

    #[test]
    fn test_needs_reattesation() {
        let mut attestation_meta = AttestationMetadata::default();

        // No attestation details = needs attestation
        assert!(attestation_meta.needs_reattesation(30));

        // Fresh attestation = doesn't need
        attestation_meta.attestation = Some(AttestationDetails {
            certificate_hash: "hash".to_string(),
            device_trusted: true,
            verified_at: Utc::now(),
            verdict: None,
            last_challenge: None,
            attestation_count: 1,
            warnings: None,
        });
        assert!(!attestation_meta.needs_reattesation(30));

        // Old attestation = needs
        attestation_meta.attestation.as_mut().unwrap().verified_at =
            Utc::now() - chrono::Duration::days(31);
        assert!(attestation_meta.needs_reattesation(30));
    }
}
