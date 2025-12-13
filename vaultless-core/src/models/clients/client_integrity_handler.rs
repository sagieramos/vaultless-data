use crate::error::{Result, VaultlessError};
use crate::models::app_model::integrity::AttestationResult;
use crate::models::app_model::integrity::Platform;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as jsonValue;
use sqlx::types::chrono::DateTime;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttestationRecord {
    pub attested_at: DateTime<Utc>,
    pub trust_score_percent: u8,

    #[serde(default, skip_serializing_if = "jsonValue::is_null")]
    pub extra: jsonValue,
}

impl From<AttestationResult> for AttestationRecord {
    fn from(result: AttestationResult) -> Self {
        AttestationRecord {
            attested_at: result.verified_at,
            trust_score_percent: result.trust_score_percent,
            extra: result.extra,
        }
    }
}

/// Holds per-platform attestation results for a client
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientIntegrityHandler {
    /// Map: platform string ("ios", "android", "iot", "browser") -> AttestationRecord
    pub platforms: HashMap<String, AttestationRecord>,
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
        self.platforms.get(platform.as_str())
    }

    /// Check if a platform needs re-attestation based on a maximum age (days)
    pub fn platform_requires_reattestation(
        &self,
        platform: Platform,
        min_trust_score: u8,
        max_age_days: u32,
    ) -> bool {
        match self.get_platform(platform) {
            Some(record) => {
                let age_days = Utc::now()
                    .signed_duration_since(record.attested_at)
                    .num_days();
                record.trust_score_percent < min_trust_score || age_days >= max_age_days as i64
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
