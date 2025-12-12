use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as jsonValue;
use validator::{Validate, ValidationErrors};

// =============================================================================
// PLATFORM ENUM
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

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// PLATFORM-SPECIFIC DATA
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IOSData {
    #[validate(length(min = 32, max = 8192))]
    pub attestation_token: String,

    /// iOS version reported by client (e.g., "17.2.1")
    #[validate(length(min = 1, max = 8))]
    pub ios_version: String,

    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    #[validate(length(min = 1, max = 255))]
    pub bundle_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    #[validate(length(min = 1, max = 255))]
    pub team_id: Option<String>,

    #[validate(length(min = 1, max = 10))]
    pub device_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AndroidData {
    #[validate(length(min = 32, max = 8192))]
    pub attestation_token: String,

    #[serde(skip_deserializing)]
    #[validate(length(min = 1, max = 255))]
    pub package_name: String,

    #[serde(skip_deserializing)]
    #[validate(length(min = 64, max = 64))]
    pub certificate_sha256: String,
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
    pub device_cn: String,

    #[validate(length(min = 100, max = 8192))]
    pub device_certificate: String,

    #[validate(length(min = 1, max = 255))]
    pub firmware_version: String,

    #[validate(length(min = 32, max = 8192))]
    pub device_signature: String,
}

// =============================================================================
// UNIFIED PLATFORM ENUM
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
    pub fn platform(&self) -> Platform {
        match self {
            PlatformAttestationData::IOS(_) => Platform::IOS,
            PlatformAttestationData::Android(_) => Platform::Android,
            PlatformAttestationData::Browser(_) => Platform::Browser,
            PlatformAttestationData::IoT(_) => Platform::IoT,
        }
    }
}

/// Manual validation for enum variants
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
// ATTESTATION REQUEST
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AttestationRequest {
    #[validate(nested)]
    pub platform_data: PlatformAttestationData,

    #[validate(length(min = 8, max = 128))]
    pub challenge: String,

    #[validate(length(min = 8, max = 1024))]
    pub challenge_signature: Option<String>,
}

// =============================================================================
// ATTESTATION RESULT
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationResult {
    pub is_valid: bool,
    pub device_trusted: bool,

    pub trust_score_percent: u8,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,

    pub verified_at: DateTime<Utc>,

    #[serde(default, skip_serializing_if = "jsonValue::is_null")]
    pub extra: jsonValue,
}
