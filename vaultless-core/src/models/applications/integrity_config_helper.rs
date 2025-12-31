use crate::models::applications::integrity::dto::*;

impl IntegrityConfig {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn dev_mode() -> Self {
        Self {
            allow_unauthenticated: Some(true),
            browser: Some(BrowserIntegrityConfig::default()),
            ios: Some(IosIntegrityConfig::default()),
            android: Some(AndroidIntegrityConfig::default()),
            iot: Some(IoTIntegrityConfig::default()),
            rate_limits: Some(RateLimits::default()),
            allowed_platforms: Some(AllowedPlatforms {
                browser: Some(true),
                ios: Some(true),
                android: Some(true),
                iot: Some(true),
            }),
        }
    }

    pub fn browser_only(browser_config: BrowserIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: Some(false),
            browser: Some(browser_config),
            ios: Some(IosIntegrityConfig::default()),
            android: Some(AndroidIntegrityConfig::default()),
            iot: Some(IoTIntegrityConfig::default()),
            rate_limits: Some(RateLimits::default()),
            allowed_platforms: Some(AllowedPlatforms {
                browser: Some(true),
                ios: Some(false),
                android: Some(false),
                iot: Some(false),
            }),
        }
    }

    pub fn ios_only(ios_config: IosIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: Some(false),
            browser: Some(BrowserIntegrityConfig::default()),
            ios: Some(ios_config),
            android: Some(AndroidIntegrityConfig::default()),
            iot: Some(IoTIntegrityConfig::default()),
            rate_limits: Some(RateLimits::default()),
            allowed_platforms: Some(AllowedPlatforms {
                browser: Some(false),
                ios: Some(true),
                android: Some(false),
                iot: Some(false),
            }),
        }
    }

    pub fn android_only(android_config: AndroidIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: Some(false),
            browser: Some(BrowserIntegrityConfig::default()),
            ios: Some(IosIntegrityConfig::default()),
            android: Some(android_config),
            iot: Some(IoTIntegrityConfig::default()),
            rate_limits: Some(RateLimits::default()),
            allowed_platforms: Some(AllowedPlatforms {
                browser: Some(false),
                ios: Some(false),
                android: Some(true),
                iot: Some(false),
            }),
        }
    }

    pub fn iot_only(iot_config: IoTIntegrityConfig) -> Self {
        Self {
            allow_unauthenticated: Some(false),
            browser: Some(BrowserIntegrityConfig::default()),
            ios: Some(IosIntegrityConfig::default()),
            android: Some(AndroidIntegrityConfig::default()),
            iot: Some(iot_config),
            rate_limits: Some(RateLimits::default()),
            allowed_platforms: Some(AllowedPlatforms {
                browser: Some(false),
                ios: Some(false),
                android: Some(false),
                iot: Some(true),
            }),
        }
    }
}
