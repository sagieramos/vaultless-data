// Core types and configuration
pub mod android_offline;
pub mod config;
pub mod dto;
pub mod integrity_handler;
pub mod ios_version;
pub mod types;
pub mod validators;

pub mod browser;
pub mod captcha;
pub mod ios;
pub mod iot;

// Main service orchestrator
pub mod service;

// Re-export commonly used items
pub use types::{AttestationRequest, AttestationResult, Platform};

pub use config::{
    AndroidConfig, IosConfig, IotConfig, PlatformConfig, extract_android_config,
    extract_ios_config, extract_iot_config,
};

pub use service::{IntegrityService, check_integrity_rate_limit, track_failed_integrity};

// Platform-specific exports
pub use browser::{
    bind_client_to_origin, check_usage_spike, track_usage, validate_browser_request,
    validate_origin, verify_client_origin,
};
pub use captcha::{CaptchaProvider, verify_captcha as verify_captcha_token};
pub use ios::verify_ios_attestation;
pub use iot::{IoTAttestationRequest, verify_iot_certificate};

// Backward compatibility alias - AttestationService is now IntegrityService
pub type AttestationService = IntegrityService;
