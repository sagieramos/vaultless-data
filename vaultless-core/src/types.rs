use serde::{Deserialize, Deserializer, Serialize};
use sqlx::Type;
use std::str::FromStr;

/// Subscription tier enum matching the database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[sqlx(type_name = "subscription_tier", rename_all = "lowercase")]
#[serde(rename_all = "PascalCase")]
pub enum SubscriptionTier {
    Free,
    Starter,
    Pro,
    Enterprise,
}

// Custom deserializer for case-insensitive parsing
impl<'de> Deserialize<'de> for SubscriptionTier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "free" => Ok(SubscriptionTier::Free),
            "starter" => Ok(SubscriptionTier::Starter),
            "pro" => Ok(SubscriptionTier::Pro),
            "enterprise" => Ok(SubscriptionTier::Enterprise),
            _ => Err(serde::de::Error::unknown_variant(
                &s,
                &["free", "starter", "pro", "enterprise"],
            )),
        }
    }
}

impl FromStr for SubscriptionTier {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "free" => Ok(SubscriptionTier::Free),
            "starter" => Ok(SubscriptionTier::Starter),
            "pro" => Ok(SubscriptionTier::Pro),
            "enterprise" => Ok(SubscriptionTier::Enterprise),
            _ => Err(()),
        }
    }
}

impl SubscriptionTier {
    /// Get default message quota for this tier
    pub fn default_monthly_quota(&self) -> i32 {
        match self {
            Self::Free => 1_000,
            Self::Starter => 50_000,
            Self::Pro => 500_000,
            Self::Enterprise => i32::MAX, // Effectively unlimited
        }
    }

    /// Get default message retention in seconds
    pub fn default_retention_seconds(&self) -> i32 {
        match self {
            Self::Free => 604_800,          // 7 days
            Self::Starter => 2_592_000,     // 30 days
            Self::Pro => 7_776_000,         // 90 days
            Self::Enterprise => 31_536_000, // 365 days
        }
    }

    /// Get default rate limit per minute
    pub fn default_rate_limit(&self) -> i32 {
        match self {
            Self::Free => 60,
            Self::Starter => 300,
            Self::Pro => 1_000,
            Self::Enterprise => 10_000,
        }
    }

    /// Get pricing in cents per month
    pub fn monthly_price_cents(&self) -> Option<i32> {
        match self {
            Self::Free => None,
            Self::Starter => Some(2_900), // $29
            Self::Pro => Some(14_900),    // $149
            Self::Enterprise => None,     // Custom pricing
        }
    }

    /// Whether this tier has proof-based features enabled
    ///
    /// For example: cryptographic proofs, signed verifications, or
    /// zero-knowledge-based message integrity validation.
    pub fn proof_enabled(&self) -> bool {
        matches!(self, Self::Pro | Self::Enterprise)
    }

    /// Whether proof *must* be enforced (optional helper)
    pub fn proof_required(&self) -> bool {
        matches!(self, Self::Enterprise)
    }
}

impl Default for SubscriptionTier {
    fn default() -> Self {
        Self::Free
    }
}

impl std::fmt::Display for SubscriptionTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Free => write!(f, "free"),
            Self::Starter => write!(f, "starter"),
            Self::Pro => write!(f, "pro"),
            Self::Enterprise => write!(f, "enterprise"),
        }
    }
}
