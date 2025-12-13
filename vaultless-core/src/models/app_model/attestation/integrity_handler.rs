use super::dto::*;
use super::types::*;
use crate::error::{Result, VaultlessError};

pub struct IntegrityConfigHandler {
    pub config: IntegrityConfig,
    pub platform_config_version: PlatformConfigVersion,
}

impl IntegrityConfigHandler {
    pub fn new_from_jsonb(json: &serde_json::Value) -> Result<Self> {
        let pf_json = json
            .get("PlatformFingerPrint")
            .ok_or_else(|| VaultlessError::Serialization("Missing PlatformFingerPrint".into()))?;
        let platform_config_version = PlatformConfigVersion::from_json(pf_json);

        let ic_json = json
            .get("IntegrityConfig")
            .ok_or_else(|| VaultlessError::Serialization("Missing IntegrityConfig".into()))?;
        let config: IntegrityConfig = serde_json::from_value(ic_json.clone()).map_err(|e| {
            VaultlessError::Serialization(format!("Failed to parse IntegrityConfig: {}", e))
        })?;

        Ok(Self {
            config,
            platform_config_version,
        })
    }

    pub fn get_allowed_bundle_ids(&self, platform: Platform) -> Option<Vec<String>> {
        let bundle_ids = match platform {
            Platform::IOS => self.config.ios.as_ref().map(|c| &c.allowed_bundle_ids),
            Platform::Android => self
                .config
                .android
                .as_ref()
                .map(|c| &c.allowed_package_names),
            Platform::IoT => self
                .config
                .iot
                .as_ref()
                .map(|c| &c.allowed_secure_element_ids),
            Platform::Browser => return None,
        }?;

        if bundle_ids.is_empty() {
            None
        } else {
            Some(bundle_ids.clone())
        }
    }

    pub fn get_min_version_code(&self, platform: Platform) -> Option<i32> {
        match platform {
            Platform::IOS => self.config.ios.as_ref().and_then(|c| c.min_version_code),
            Platform::Android => self
                .config
                .android
                .as_ref()
                .and_then(|c| c.min_version_code),
            Platform::IoT => self
                .config
                .iot
                .as_ref()
                .and_then(|c| c.min_firmware_version),
            Platform::Browser => None,
        }
    }

    pub fn get_platform_config<'a>(&'a self, platform: Platform) -> Option<PlatformConfigRef<'a>> {
        match platform {
            Platform::IOS => self.config.ios.as_ref().map(PlatformConfigRef::IOS),
            Platform::Android => self.config.android.as_ref().map(PlatformConfigRef::Android),
            Platform::IoT => self.config.iot.as_ref().map(PlatformConfigRef::IoT),
            Platform::Browser => self.config.browser.as_ref().map(PlatformConfigRef::Browser),
        }
    }

    pub fn get_android_config(&self) -> Option<&AndroidIntegrityConfig> {
        self.config.android.as_ref()
    }

    pub fn get_ios_config(&self) -> Option<&IosIntegrityConfig> {
        self.config.ios.as_ref()
    }

    pub fn get_iot_config(&self) -> Option<&IoTIntegrityConfig> {
        self.config.iot.as_ref()
    }

    pub fn get_browser_config(&self) -> Option<&BrowserIntegrityConfig> {
        self.config.browser.as_ref()
    }

    pub fn get_rate_limits(&self) -> Option<&RateLimits> {
        self.config.rate_limits.as_ref()
    }

    /// Get trust score and reattestation days for a given platform
    /// Returns (trust_score, reattestation_days)
    pub fn get_trust_score_and_reattestation(&self, platform: Platform) -> (u8, u32) {
        match platform {
            Platform::Browser => {
                let trust_score = self
                    .config
                    .browser
                    .as_ref()
                    .map(|c| c.calculate_trust_score())
                    .unwrap_or(0);
                (trust_score, 0)
            }
            Platform::IOS => {
                let ios_config = self.config.ios.as_ref();
                let trust_score = ios_config.map(|c| c.calculate_trust_score()).unwrap_or(0);
                let reattestation = ios_config.and_then(|c| c.reattestation_days);
                (trust_score, reattestation.unwrap_or(0))
            }
            Platform::Android => {
                let android_config = self.config.android.as_ref();
                let trust_score = android_config
                    .map(|c| c.calculate_trust_score())
                    .unwrap_or(0);
                let reattestation = android_config.and_then(|c| c.reattestation_days);
                (trust_score, reattestation.unwrap_or(0))
            }
            Platform::IoT => {
                let iot_config = self.config.iot.as_ref();
                let trust_score = iot_config.map(|c| c.calculate_trust_score()).unwrap_or(0);
                let reattestation = iot_config.and_then(|c| c.reattestation_days);
                (trust_score, reattestation.unwrap_or(0))
            }
        }
    }
}

pub enum PlatformConfigRef<'a> {
    Browser(&'a BrowserIntegrityConfig),
    Android(&'a AndroidIntegrityConfig),
    IOS(&'a IosIntegrityConfig),
    IoT(&'a IoTIntegrityConfig),
}

impl<'a> PlatformConfigRef<'a> {
    pub fn reattestation_days(&self) -> Option<u32> {
        match self {
            PlatformConfigRef::Browser(_) => None,
            PlatformConfigRef::Android(cfg) => cfg.reattestation_days,
            PlatformConfigRef::IOS(cfg) => cfg.reattestation_days,
            PlatformConfigRef::IoT(cfg) => cfg.reattestation_days,
        }
    }

    pub fn trust_score(&self) -> u8 {
        match self {
            PlatformConfigRef::Browser(cfg) => cfg.calculate_trust_score(),
            PlatformConfigRef::Android(cfg) => cfg.calculate_trust_score(),
            PlatformConfigRef::IOS(cfg) => cfg.calculate_trust_score(),
            PlatformConfigRef::IoT(cfg) => cfg.calculate_trust_score(),
        }
    }

    pub fn trust_score_and_reattestation(&self) -> (u8, Option<u32>) {
        (self.trust_score(), self.reattestation_days())
    }
}
