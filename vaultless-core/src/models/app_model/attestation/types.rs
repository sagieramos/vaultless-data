use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use validator::{Validate, ValidationErrors};

// =============================================================================
// PLATFORM ENUM (simple enum identifying type)
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    IOS,
    Android,
    Browser,
    IoT,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::IOS => "ios",
            Platform::Android => "android",
            Platform::Browser => "browser",
            Platform::IoT => "iot",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// PLATFORM-SPECIFIC STRUCTS
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IOSData {
    #[validate(length(min = 1, max = 255))]
    pub app_id: String,

    #[validate(length(min = 32, max = 8192))]
    pub attestation_token: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_info: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AndroidData {
    #[validate(length(min = 1, max = 255))]
    pub package_name: String,

    #[validate(length(min = 64, max = 64))]
    pub certificate_sha256: String,

    #[validate(length(min = 32, max = 8192))]
    pub attestation_token: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_info: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct BrowserData {
    #[validate(url)]
    pub origin: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IoTData {
    #[validate(length(min = 1, max = 255))]
    pub device_id: String,

    #[validate(length(min = 1, max = 255))]
    pub firmware_version: String,

    #[validate(length(min = 32, max = 8192))]
    pub attestation_token: String,
}

// =============================================================================
// UNIFIED ENUM
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "lowercase")]
pub enum PlatformAttestationData {
    IOS(IOSData),
    Android(AndroidData),
    Browser(BrowserData),
    IoT(IoTData),
}

impl PlatformAttestationData {
    /// returns the platform
    pub fn platform(&self) -> Platform {
        match self {
            PlatformAttestationData::IOS(_) => Platform::IOS,
            PlatformAttestationData::Android(_) => Platform::Android,
            PlatformAttestationData::Browser(_) => Platform::Browser,
            PlatformAttestationData::IoT(_) => Platform::IoT,
        }
    }
}

/// Manual validation
impl Validate for PlatformAttestationData {
    fn validate(&self) -> std::result::Result<(), ValidationErrors> {
        match self {
            PlatformAttestationData::IOS(d) => d.validate(),
            PlatformAttestationData::Android(d) => d.validate(),
            PlatformAttestationData::Browser(d) => d.validate(),
            PlatformAttestationData::IoT(d) => d.validate(),
        }
    }
}

// =============================================================================
// REQUEST + RESULT TYPES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlatformResultData {
    IOS {
        team_id: String,
        app_id: String,
        certificate_chain: Vec<String>,
    },

    Android {
        device_integrity: bool,
        account_integrity: bool,
        app_integrity: bool,
        nonce: String,
    },

    IoT {
        firmware_hash: String,
        public_key_fingerprint: String,
    },

    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AttestationRequest {
    #[validate(nested)]
    pub platform_data: PlatformAttestationData,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 8, max = 128))]
    pub challenge: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50))]
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationResult {
    pub is_valid: bool,
    pub certificate_hash: String,
    pub client_app_id: String,
    pub platform: Platform,
    pub device_trusted: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,

    pub verified_at: DateTime<Utc>,
}

// =============================================================================
// METADATA
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
    pub model: Option<String>,
    pub os_version: Option<String>,
    pub manufacturer: Option<String>,
    pub additional: Option<serde_json::Value>,
}
