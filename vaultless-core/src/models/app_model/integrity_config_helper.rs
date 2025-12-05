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
            iot: IoTIntegrityConfig::development(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn browser_only(browser_config: self::BrowserIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: browser_config,
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn ios_only(ios_config: self::IosIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: ios_config,
            android: AndroidIntegrityConfig::default(),
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn android_only(android_config: self::AndroidIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: android_config,
            iot: IoTIntegrityConfig::default(),
            rate_limits: RateLimits::default(),
        }
    }

    pub fn iot_only(iot_config: self::IoTIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: false,
            browser: BrowserIntegrityConfig::default(),
            ios: IosIntegrityConfig::default(),
            android: AndroidIntegrityConfig::default(),
            iot: iot_config,
            rate_limits: RateLimits::default(),
        }
    }
}
