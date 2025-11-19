// Core types and configuration
pub mod types;
pub mod config;
pub mod dto;

// Platform-specific validators
pub mod android;
pub mod ios;
pub mod iot;
pub mod browser;
pub mod captcha;

// Main service orchestrator
pub mod service;

// Re-export commonly used items
pub use types::{
    AttestationMetadata, AttestationRequest, AttestationResult, DeviceInfo, Platform,
};

pub use config::{
    AndroidConfig, IosConfig, IotConfig, PlatformConfig,
    extract_android_config, extract_ios_config, extract_iot_config,
};

pub use service::{
    AttestationService, check_attestation_rate_limit, track_failed_attestation,
};

// Platform-specific exports
pub use android::verify_android_attestation;
pub use ios::{verify_ios_attestation, generate_ios_challenge};
pub use iot::{verify_iot_certificate, generate_iot_challenge, IoTAttestationRequest};
pub use browser::{
    validate_browser_request, validate_origin, bind_client_to_origin, 
    verify_client_origin, track_usage, check_usage_spike,
};
pub use captcha::{CaptchaProvider, verify_captcha as verify_captcha_token};