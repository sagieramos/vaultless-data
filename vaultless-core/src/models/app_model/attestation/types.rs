use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use validator::Validate;

use crate::error::{Result, VaultlessError};

// =============================================================================
// PLATFORM ENUM
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
    #[serde(rename = "iot")]
    IoT,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::IOS => "ios",
            Platform::Android => "android",
            Platform::Web => "web",
            Platform::IoT => "iot",
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "ios" => Ok(Platform::IOS),
            "android" => Ok(Platform::Android),
            "web" => Ok(Platform::Web),
            "iot" => Ok(Platform::IoT),
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
// ATTESTATION REQUEST/RESPONSE
// =============================================================================

/// Request to verify platform attestation during registration or re-attestation
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AttestationRequest {
    /// Platform being attested
    pub platform: Platform,

    /// Bundle ID or package name (device_id for IoT)
    #[validate(length(min = 1, max = 255))]
    pub bundle_id: String,

    /// Device identifier
    #[validate(length(min = 1, max = 255))]
    pub device_id: String,

    /// The attestation token from the platform
    #[validate(length(min = 32, max = 8192))]
    pub attestation_token: String,

    /// Challenge/nonce for replay protection (REQUIRED for iOS/IoT)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 8, max = 128))]
    pub challenge: Option<String>,

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

    /// Verified bundle ID / device ID
    pub bundle_id: String,

    /// Platform that was attested
    pub platform: Platform,

    /// Whether device is trusted
    pub device_trusted: bool,

    /// Platform-specific verdict
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
// METADATA (stored in Client.metadata JSONB)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttestationMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<AttestationDetails>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_info: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationDetails {
    pub certificate_hash: String,
    pub device_trusted: bool,
    pub verified_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_challenge: Option<String>,

    #[serde(default)]
    pub attestation_count: u32,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional: Option<serde_json::Value>,
}

// =============================================================================
// METADATA HELPER METHODS
// =============================================================================

impl AttestationMetadata {
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

    pub fn needs_reattesation(&self, days: i64) -> bool {
        match &self.attestation {
            Some(att) => {
                let age = Utc::now().signed_duration_since(att.verified_at).num_days();
                age > days
            }
            None => true,
        }
    }

    pub fn is_device_trusted(&self) -> bool {
        self.attestation
            .as_ref()
            .map(|a| a.device_trusted)
            .unwrap_or(false)
    }

    pub fn merge_into_metadata(
        &self,
        existing: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let mut metadata = existing.unwrap_or(serde_json::json!({}));

        if let Some(obj) = metadata.as_object_mut() {
            let attestation_value = serde_json::to_value(self)
                .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

            if let Some(platform) = &self.platform {
                obj.insert("platform".to_string(), serde_json::json!(platform));
            }
            if let Some(bundle_id) = &self.bundle_id {
                obj.insert("bundle_id".to_string(), serde_json::json!(bundle_id));
            }
            if let Some(device_id) = &self.device_id {
                obj.insert("device_id".to_string(), serde_json::json!(device_id));
            }

            obj.insert("attestation".to_string(), attestation_value);
        }

        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_serialization() {
        assert_eq!(Platform::IOS.as_str(), "ios");
        assert_eq!(Platform::Android.as_str(), "android");
        assert_eq!(Platform::Web.as_str(), "web");
        assert_eq!(Platform::IoT.as_str(), "iot");

        assert_eq!(Platform::from_str("iOS").unwrap(), Platform::IOS);
        assert_eq!(Platform::from_str("ANDROID").unwrap(), Platform::Android);
        assert!(Platform::from_str("invalid").is_err());
    }

    #[test]
    fn test_needs_reattesation() {
        let mut meta = AttestationMetadata::default();
        assert!(meta.needs_reattesation(30));

        meta.attestation = Some(AttestationDetails {
            certificate_hash: "test".to_string(),
            device_trusted: true,
            verified_at: Utc::now(),
            verdict: None,
            last_challenge: None,
            attestation_count: 1,
            warnings: None,
        });
        assert!(!meta.needs_reattesation(30));

        meta.attestation.as_mut().unwrap().verified_at =
            Utc::now() - chrono::Duration::days(31);
        assert!(meta.needs_reattesation(30));
    }
}