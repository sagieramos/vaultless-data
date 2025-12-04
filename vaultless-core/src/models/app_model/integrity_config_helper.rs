use crate::models::app_model::attestation::dto::*;

impl IntegrityConfig {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn dev_mode() -> Self {
        Self {
            allow_unauthenticated: true,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn browser_only(authorized_origins: Vec<String>) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig {
                authorized_origins,
                ..Default::default()
            },
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn ios_only(
        apple_team_id: String,
        bundle_ids: Vec<String>,
        reject_untrusted: bool,
    ) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig {
                apple_team_id: Some(apple_team_id),
                allowed_bundle_ids: bundle_ids,
                allowed_certificate_hashes: vec![],
                min_version_code: None,
                reject_untrusted_device: reject_untrusted,
                challenge_ttl_seconds: 60,
            },
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn android_only(
        cert_hash: String,
        bundle_ids: Vec<String>,
        google_cloud_project: String,
        google_api_key: String,
        reject_untrusted: bool,
    ) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig {
                allowed_certificate_sha256: Some(cert_hash),
                allowed_bundle_ids: bundle_ids,
                min_version_code: None,
                reject_untrusted_device: reject_untrusted,
                reject_unrecognized_version: true,
                google_cloud_project: Some(google_cloud_project),
                google_api_key: Some(google_api_key),
                max_token_age_seconds: 60,
            },
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn iot_only(
        allowed_cas: Vec<String>,
        allowed_device_ids: Vec<String>,
        require_cn_match: bool,
    ) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig {
                require_device_certificate: true,
                allowed_certificate_authorities: allowed_cas,
                allowed_device_ids,
                min_firmware_version: None,
                challenge_ttl_seconds: 30,
                require_cn_match,
            },
            rate_limits: RateLimits::default(),
        }
    }
}
