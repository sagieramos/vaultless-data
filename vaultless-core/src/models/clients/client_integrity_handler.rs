use crate::error::{Result, VaultlessError};
use crate::models::applications::integrity::AttestationResult;
use crate::models::applications::integrity::Platform;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as jsonValue;
use sqlx::types::chrono::DateTime;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttestationRecord {
    pub attested_at: DateTime<Utc>,
    pub trust_score_percent: u8,

    #[serde(default, skip_serializing_if = "jsonValue::is_null")]
    pub extra: jsonValue,

    platform_version: Uuid,
}

impl AttestationResult {
    pub fn into_record(self, current_platform_version: Uuid) -> AttestationRecord {
        AttestationRecord {
            attested_at: self.verified_at,
            trust_score_percent: self.trust_score_percent,
            extra: self.extra,
            platform_version: current_platform_version,
        }
    }
}

/// Holds per-platform attestation results for a client
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientIntegrityHandler {
    /// Map: platform string ("ios", "android", "iot", "browser") -> AttestationRecord
    pub platforms: HashMap<Platform, AttestationRecord>,
}

impl ClientIntegrityHandler {
    pub fn new(metadata_json: &Option<serde_json::Value>) -> Result<Self> {
        if let Some(json) = metadata_json {
            let handler: ClientIntegrityHandler =
                serde_json::from_str(&json.to_string()).map_err(|e| {
                    VaultlessError::Serialization(format!("Failed to parse client metadata: {}", e))
                })?;
            Ok(handler)
        } else {
            Ok(ClientIntegrityHandler::default())
        }
    }

    /// Returns the attestation record for a specific platform
    pub fn get_platform(&self, platform: Platform) -> Option<&AttestationRecord> {
        self.platforms.get(&platform)
    }

    /// Check if a platform needs re-attestation based on a maximum age (days)
    /// and current app platform config version
    pub fn platform_requires_reattestation(
        &self,
        platform: Platform,
        min_trust_score: u8,
        max_age_days: u32,
        current_platform_version: Uuid,
    ) -> bool {
        match self.get_platform(platform) {
            Some(record) => {
                let age_days = Utc::now()
                    .signed_duration_since(record.attested_at)
                    .num_days();

                let trust_failed = record.trust_score_percent < min_trust_score;
                let expired = age_days >= max_age_days as i64;
                let version_mismatch = record.platform_version != current_platform_version;

                trust_failed || expired || version_mismatch
            }
            None => true,
        }
    }

    pub fn get_platform_trust_score(&self, platform: Platform) -> Option<u8> {
        self.get_platform(platform)
            .map(|record| record.trust_score_percent)
    }
}

impl super::dto::Client {
    pub fn integrity(&self) -> Result<ClientIntegrityHandler> {
        ClientIntegrityHandler::new(&self.metadata)
    }
}
