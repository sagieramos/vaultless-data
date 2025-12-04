// Core types and configuration
pub mod andriod_offline;
pub mod config;
pub mod dto;
pub mod integrity_handler;
pub mod types;
pub mod validators;

pub mod android_online;
pub mod browser;
pub mod captcha;
pub mod ios;
pub mod iot;

// Main service orchestrator
pub mod service;

// Re-export commonly used items
pub use types::{AttestationMetadata, AttestationRequest, AttestationResult, DeviceInfo, Platform};

pub use config::{
    AndroidConfig, IosConfig, IotConfig, PlatformConfig, extract_android_config,
    extract_ios_config, extract_iot_config,
};

pub use service::{AttestationService, check_attestation_rate_limit, track_failed_attestation};

// Platform-specific exports
pub use android_online::verify_android_attestation_online;
pub use browser::{
    bind_client_to_origin, check_usage_spike, track_usage, validate_browser_request,
    validate_origin, verify_client_origin,
};
pub use captcha::{CaptchaProvider, verify_captcha as verify_captcha_token};
pub use ios::{generate_ios_challenge, verify_ios_attestation};
pub use iot::{IoTAttestationRequest, generate_iot_challenge, verify_iot_certificate};
