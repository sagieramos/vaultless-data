//! Metric counter types and Redis key management for clients.

use crate::cache_key;
use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use utoipa::ToSchema;
use uuid::Uuid;

// =============================================================================
// Time Utilities
// =============================================================================

/// Truncate a DateTime<Utc> to the start of the current hour
#[inline(always)]
pub fn get_hour_window(now: &DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(now.hour(), 0, 0)
        .map(|dt| dt.and_utc())
        .unwrap_or(*now)
}

/// Truncate a DateTime<Utc> to the start of the current minute
#[inline(always)]
pub fn get_minute_window(now: &DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(now.hour(), now.minute(), 0)
        .map(|dt| dt.and_utc())
        .unwrap_or(*now)
}

// =============================================================================
// Metric Granularity
// =============================================================================

/// Time granularity for metric aggregation
#[derive(Debug, Clone, Copy)]
pub enum MetricGranularity {
    Hour,
    Minute,
}

// =============================================================================
// ClientMetricKey - Redis Key Newtype
// =============================================================================

/// Strongly-typed wrapper for Redis client metric keys
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientMetricKey(String);

impl ClientMetricKey {
    /// Create a new metric key for the given application, client, and time period
    pub fn new(
        application_id: Uuid,
        client_id: Uuid,
        now: DateTime<Utc>,
        granularity: MetricGranularity,
    ) -> Result<Self> {
        let period_start = match granularity {
            MetricGranularity::Hour => get_hour_window(&now),
            MetricGranularity::Minute => get_minute_window(&now),
        };

        let mut buf = String::with_capacity(12); // YYYYMMDDHHMM

        match granularity {
            MetricGranularity::Hour => write!(
                &mut buf,
                "{:04}{:02}{:02}{:02}",
                period_start.year(),
                period_start.month(),
                period_start.day(),
                period_start.hour()
            )
            .unwrap(),
            MetricGranularity::Minute => write!(
                &mut buf,
                "{:04}{:02}{:02}{:02}{:02}",
                period_start.year(),
                period_start.month(),
                period_start.day(),
                period_start.hour(),
                period_start.minute()
            )
            .unwrap(),
        }

        Ok(Self(cache_key!(
            "metric",
            match granularity {
                MetricGranularity::Hour => "client_hour",
                MetricGranularity::Minute => "client_minute",
            },
            application_id,
            client_id,
            buf
        )))
    }

    /// Get the string representation of the key
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse the key to extract application ID, client ID, and period start
    pub fn parse(&self) -> Option<(Uuid, Uuid, DateTime<Utc>)> {
        let prefix_hour = cache_key!("metric", "client_hour");
        let prefix_minute = cache_key!("metric", "client_minute");

        let s = if self.0.starts_with(&prefix_hour) {
            &self.0[prefix_hour.len()..]
        } else if self.0.starts_with(&prefix_minute) {
            &self.0[prefix_minute.len()..]
        } else {
            return None;
        };

        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return None;
        }

        let app_id = Uuid::parse_str(parts[0]).ok()?;
        let client_id = Uuid::parse_str(parts[1]).ok()?;
        let timestamp_str = parts[2];

        let (year, month, day, hour, minute) = if timestamp_str.len() == 10 { // YYYYMMDDHH
            (
                timestamp_str[0..4].parse().ok()?,
                timestamp_str[4..6].parse().ok()?,
                timestamp_str[6..8].parse().ok()?,
                timestamp_str[8..10].parse().ok()?,
                0,
            )
        } else if timestamp_str.len() == 12 { // YYYYMMDDHHMM
            (
                timestamp_str[0..4].parse().ok()?,
                timestamp_str[4..6].parse().ok()?,
                timestamp_str[6..8].parse().ok()?,
                timestamp_str[8..10].parse().ok()?,
                timestamp_str[10..12].parse().ok()?,
            )
        } else {
            return None;
        };

        let naive = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, 0)?;
        Some((app_id, client_id, naive.and_utc()))
    }
}

impl TryFrom<String> for ClientMetricKey {
    type Error = VaultlessError;

    fn try_from(s: String) -> Result<Self> {
        if !s.starts_with(&cache_key!("metric", "client")) {
            return Err(VaultlessError::InvalidInput("Invalid client metric key format".into()));
        }

        let key = Self(s);
        if key.parse().is_none() {
            return Err(VaultlessError::InvalidInput("Failed to parse client metric key".into()));
        }
        Ok(key)
    }
}

// =============================================================================
// ClientMetricCounters - In-Memory Counter State
// =============================================================================

/// In-memory representation of client metric counters for a single period
#[derive(Debug, Default, Clone, Serialize, ToSchema)]
pub struct ClientMetricCounters {
    #[schema(example = 100)]
    pub messages_sent: i64,
    #[schema(example = 50)]
    pub messages_received: i64,
    #[schema(example = 25)]
    pub proofs_verified: i64,
    #[schema(example = 102400)]
    pub total_bytes_sent: i64,
    #[schema(example = 51200)]
    pub total_bytes_received: i64,
    #[schema(example = 5)]
    pub rate_limit_hits: i64,
}

impl ClientMetricCounters {
    /// Check if all counters are zero
    pub fn is_zero(&self) -> bool {
        self.messages_sent == 0
            && self.messages_received == 0
            && self.proofs_verified == 0
            && self.total_bytes_sent == 0
            && self.total_bytes_received == 0
            && self.rate_limit_hits == 0
    }

    /// Merge values from a Redis hash map into this counter
    pub fn merge_from_map(&mut self, map: &HashMap<String, i64>) {
        self.messages_sent += map.get("messages_sent").unwrap_or(&0);
        self.messages_received += map.get("messages_received").unwrap_or(&0);
        self.proofs_verified += map.get("proofs_verified").unwrap_or(&0);
        self.total_bytes_sent += map.get("total_bytes_sent").unwrap_or(&0);
        self.total_bytes_received += map.get("total_bytes_received").unwrap_or(&0);
        self.rate_limit_hits += map.get("rate_limit_hits").unwrap_or(&0);
    }

    /// Estimate cost in cents based on usage
    pub fn estimate_cost_cents(&self) -> i64 {
        // Note: These costs are illustrative. Real costs depend on the billing plan.
        let message_cost = (self.messages_sent as f64 / 1000.0) * 0.5; // $0.50 per 1k messages
        let total_bytes = self.total_bytes_sent + self.total_bytes_received;
        let transfer_cost = (total_bytes as f64 / 1_000_000_000.0) * 5.0; // $5 per GB
        let proof_cost = (self.proofs_verified as f64 / 1000.0) * 0.05; // $0.05 per 1k proofs

        ((message_cost + transfer_cost + proof_cost) * 100.0).round() as i64
    }
}

// =============================================================================
// ClientFlusherMetrics - Monitoring Counters
// =============================================================================

/// Metrics for monitoring the background flusher performance for clients
#[derive(Debug)]
pub struct ClientFlusherMetrics {
    pub keys_processed: AtomicU64,
    pub errors: AtomicU64,
    pub batches_processed: AtomicU64,
    pub total_flush_duration_ms: AtomicU64,
}

impl Default for ClientFlusherMetrics {
    fn default() -> Self {
        Self {
            keys_processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            batches_processed: AtomicU64::new(0),
            total_flush_duration_ms: AtomicU64::new(0),
        }
    }
}

impl ClientFlusherMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::SeqCst);
    }

    pub fn average_flush_duration_ms(&self) -> f64 {
        let batches = self.batches_processed.load(Ordering::SeqCst);
        let duration = self.total_flush_duration_ms.load(Ordering::SeqCst);

        if batches > 0 {
            duration as f64 / batches as f64
        } else {
            0.0
        }
    }
}
