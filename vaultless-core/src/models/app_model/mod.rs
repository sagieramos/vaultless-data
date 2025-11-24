pub mod application;
pub mod attestation;
mod cache_resolved_key_bundled;
pub mod dto;
pub mod integrity_config;
pub mod integrity_config_handler;
pub mod invalidate_cache;
pub mod resolve;
pub mod update;
pub mod helper;

pub use dto::{
    Application, ApplicationWithTier, AuthConfig, CreateApplication, CreateApplicationResponse,
    KeyGranularity, UpdateApplication,
};

use attestation::service::{
    AttestationService, check_attestation_rate_limit, track_failed_attestation,
};
