//! Shared counter types for usage metrics.
//!
//! Provides common structures for both application-level and client-level metrics.

use crate::cache_key;
use crate::error::VaultlessError;
use chrono::{DateTime, Datelike, Timelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::Hash;
use std::str::FromStr;
use uuid::Uuid;

// =============================================================================
// Metric Granularity
// =============================================================================

/// Granularity levels for metric aggregation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricGranularity {
    Minute,
    Hour,
    Day,
    Month,
}

impl MetricGranularity {
    /// Get the Redis key suffix for this granularity
    pub fn key_suffix(&self, dt: &DateTime<Utc>) -> String {
        match self {
            Self::Minute => format!("{:04}_{:02}_{:02}_{:02}_{:02}", dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute()),
            Self::Hour => format!("{:04}_{:02}_{:02}_{:02}", dt.year(), dt.month(), dt.day(), dt.hour()),
            Self::Day => format!("{:04}_{:02}_{:02}", dt.year(), dt.month(), dt.day()),
            Self::Month => format!("{:04}_{:02}", dt.year(), dt.month()),
        }
    }

    /// Get the TTL multiplier for this granularity
    pub fn ttl_multiplier_hours(&self) -> i64 {
        match self {
            Self::Minute => 1,
            Self::Hour => 2,
            Self::Day => 48,
            Self::Month => 720,
        }
    }

    fn key_prefix(&self) -> &'static str {
        match self {
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Month => "month",
        }
    }
}

impl FromStr for MetricGranularity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "minute" | "min" => Ok(Self::Minute),
            "hour" | "hr" => Ok(Self::Hour),
            "day" | "d" => Ok(Self::Day),
            "month" | "mon" => Ok(Self::Month),
            _ => Err(format!("Unknown metric granularity: {}", s)),
        }
    }
}

// =============================================================================
// Metric Key
// =============================================================================

/// Key structure for application-level metrics
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricKey {
    pub app_id: Uuid,
    pub granularity: MetricGranularity,
    pub window: DateTime<Utc>,
}

impl MetricKey {
    /// Create a new metric key
    pub fn new(app_id: Uuid, dt: DateTime<Utc>, granularity: MetricGranularity) -> Result<Self, VaultlessError> {
        let window = match granularity {
            MetricGranularity::Minute => {
                Utc.timestamp_opt(dt.timestamp() / 60 * 60, 0).single().unwrap_or(dt)
            }
            MetricGranularity::Hour => {
                Utc.timestamp_opt(dt.timestamp() / 3600 * 3600, 0).single().unwrap_or(dt)
            }
            MetricGranularity::Day => {
                Utc.timestamp_opt(dt.timestamp() / 86400 * 86400, 0).single().unwrap_or(dt)
            }
            MetricGranularity::Month => {
                Utc.with_ymd_and_hms(dt.year(), dt.month() as u32, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(dt)
            }
        };

        Ok(Self { app_id, granularity, window })
    }

    /// Get the Redis key string
    pub fn as_str(&self) -> String {
        let suffix = self.granularity.key_suffix(&self.window);
        cache_key!("metric", "app", self.app_id, self.granularity.key_prefix(), suffix)
    }
}

impl fmt::Display for MetricKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl MetricKey {
    /// Parse from a Redis key string
    pub fn try_from(s: String) -> Result<Self, VaultlessError> {
        // Expected format: metric:app:{app_id}:{granularity}:{window}
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() < 5 {
            return Err(VaultlessError::Internal(format!("Invalid metric key format: {}", s)));
        }

        let app_id = Uuid::from_str(parts[2]).map_err(|_| VaultlessError::Internal("Invalid app_id".into()))?;
        let granularity = MetricGranularity::from_str(parts[3]).map_err(|e| VaultlessError::Internal(e))?;
        let window_str = parts[4..].join(":");

        // Parse window based on granularity
        let window = match granularity {
            MetricGranularity::Minute => {
                let nums: Vec<i32> = window_str.split('_').map(|s| s.parse::<i32>().map_err(|e| VaultlessError::Internal(e.to_string()))).collect::<Result<_, _>>()?;
                if nums.len() != 5 { return Err(VaultlessError::Internal("Invalid minute window".into())); }
                Utc.with_ymd_and_hms(nums[0], nums[1] as u32, nums[2] as u32, nums[3] as u32, nums[4] as u32, 0)
                    .single().ok_or(VaultlessError::Internal("Invalid datetime".into()))?
            }
            MetricGranularity::Hour => {
                let nums: Vec<i32> = window_str.split('_').map(|s| s.parse::<i32>().map_err(|e| VaultlessError::Internal(e.to_string()))).collect::<Result<_, _>>()?;
                if nums.len() != 4 { return Err(VaultlessError::Internal("Invalid hour window".into())); }
                Utc.with_ymd_and_hms(nums[0], nums[1] as u32, nums[2] as u32, nums[3] as u32, 0, 0)
                    .single().ok_or(VaultlessError::Internal("Invalid datetime".into()))?
            }
            _ => return Err(VaultlessError::Internal("Unsupported granularity for parsing".into())),
        };

        Ok(Self::new(app_id, window, granularity)?)
    }

    /// Parse key and extract application_id and period
    pub fn parse(&self) -> Option<(Uuid, DateTime<Utc>)> {
        Some((self.app_id, self.window))
    }
}

// =============================================================================
// Client Metric Key
// =============================================================================

/// Key structure for client-level metrics
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientMetricKey {
    pub app_id: Uuid,
    pub client_id: Uuid,
    pub granularity: MetricGranularity,
    pub window: DateTime<Utc>,
}

impl ClientMetricKey {
    /// Create a new client metric key
    pub fn new(app_id: Uuid, client_id: Uuid, dt: DateTime<Utc>, granularity: MetricGranularity) -> Result<Self, VaultlessError> {
        let window = match granularity {
            MetricGranularity::Minute => {
                Utc.timestamp_opt(dt.timestamp() / 60 * 60, 0).single().unwrap_or(dt)
            }
            MetricGranularity::Hour => {
                Utc.timestamp_opt(dt.timestamp() / 3600 * 3600, 0).single().unwrap_or(dt)
            }
            _ => dt,
        };

        Ok(Self { app_id, client_id, granularity, window })
    }

    /// Get the Redis key string
    pub fn as_str(&self) -> String {
        let suffix = self.granularity.key_suffix(&self.window);
        cache_key!("metric", "client", self.app_id, self.client_id, self.granularity.key_prefix(), suffix)
    }
}

impl fmt::Display for ClientMetricKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl ClientMetricKey {
    /// Parse from a Redis key string
    pub fn try_from(s: String) -> Result<Self, VaultlessError> {
        // Expected format: metric:client:{app_id}:{client_id}:{granularity}:{window}
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() < 6 {
            return Err(VaultlessError::Internal(format!("Invalid client metric key format: {}", s)));
        }

        let app_id = Uuid::from_str(parts[2]).map_err(|_| VaultlessError::Internal("Invalid app_id".into()))?;
        let client_id = Uuid::from_str(parts[3]).map_err(|_| VaultlessError::Internal("Invalid client_id".into()))?;
        let granularity = MetricGranularity::from_str(parts[4]).map_err(|e| VaultlessError::Internal(e))?;
        let window_str = parts[5..].join(":");

        // Parse window based on granularity
        let window = match granularity {
            MetricGranularity::Hour => {
                let nums: Vec<i32> = window_str.split('_').map(|s| s.parse::<i32>().map_err(|e| VaultlessError::Internal(e.to_string()))).collect::<Result<_, _>>()?;
                if nums.len() != 4 { return Err(VaultlessError::Internal("Invalid hour window".into())); }
                Utc.with_ymd_and_hms(nums[0], nums[1] as u32, nums[2] as u32, nums[3] as u32, 0, 0)
                    .single().ok_or(VaultlessError::Internal("Invalid datetime".into()))?
            }
            _ => return Err(VaultlessError::Internal("Unsupported granularity for client metric parsing".into())),
        };

        Ok(Self::new(app_id, client_id, window, granularity)?)
    }

    /// Parse key and extract application_id, client_id, and period
    pub fn parse(&self) -> Option<(Uuid, Uuid, DateTime<Utc>)> {
        Some((self.app_id, self.client_id, self.window))
    }
}

// =============================================================================
// Hour Window Helper
// =============================================================================

/// Get the hour window cutoff for flushing
pub fn get_hour_window(now: &DateTime<Utc>) -> DateTime<Utc> {
    let timestamp = now.timestamp() / 3600 * 3600;
    Utc.timestamp_opt(timestamp, 0).single().unwrap_or(*now)
}

/// Get the minute window for rate limiting
pub fn get_minute_window(now: &DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(now.hour(), now.minute(), 0)
        .map(|dt| dt.and_utc())
        .unwrap_or(*now)
}

// =============================================================================
// Application Metric Counters
// =============================================================================

/// Counters for application-level metrics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MetricCounters {
    pub messages_sent: i64,
    pub messages_received: i64,
    pub proofs_verified: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub rate_limit_hits: i64,
    pub bytes_proved: i64,
}

impl MetricCounters {
    /// Check if all counters are zero
    pub fn is_zero(&self) -> bool {
        self.messages_sent == 0
            && self.messages_received == 0
            && self.proofs_verified == 0
            && self.total_bytes_sent == 0
            && self.total_bytes_received == 0
            && self.rate_limit_hits == 0
            && self.bytes_proved == 0
    }

    /// Merge counters from a HashMap (Redis HGETALL result)
    pub fn merge_from_map(&mut self, map: &std::collections::HashMap<String, i64>) {
        if let Some(v) = map.get("messages_sent") { self.messages_sent = *v; }
        if let Some(v) = map.get("messages_received") { self.messages_received = *v; }
        if let Some(v) = map.get("proofs_verified") { self.proofs_verified = *v; }
        if let Some(v) = map.get("total_bytes_sent") { self.total_bytes_sent = *v; }
        if let Some(v) = map.get("total_bytes_received") { self.total_bytes_received = *v; }
        if let Some(v) = map.get("rate_limit_hits") { self.rate_limit_hits = *v; }
        if let Some(v) = map.get("bytes_proved") { self.bytes_proved = *v; }
    }

    /// Estimate cost in cents based on usage
    pub fn estimate_cost_cents(&self) -> i64 {
        // Simple cost model: $0.001 per 1000 messages + $0.001 per MB
        let message_cost = (self.messages_sent + self.messages_received) / 1000;
        let byte_cost = (self.total_bytes_sent + self.total_bytes_received) / 1_000_000;
        message_cost + byte_cost
    }
}

// =============================================================================
// Client Metric Counters
// =============================================================================

/// Counters for client-level metrics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ClientMetricCounters {
    pub messages_sent: i64,
    pub messages_received: i64,
    pub proofs_verified: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
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

    /// Merge counters from a HashMap (Redis HGETALL result)
    pub fn merge_from_map(&mut self, map: &std::collections::HashMap<String, i64>) {
        if let Some(v) = map.get("messages_sent") { self.messages_sent = *v; }
        if let Some(v) = map.get("messages_received") { self.messages_received = *v; }
        if let Some(v) = map.get("proofs_verified") { self.proofs_verified = *v; }
        if let Some(v) = map.get("total_bytes_sent") { self.total_bytes_sent = *v; }
        if let Some(v) = map.get("total_bytes_received") { self.total_bytes_received = *v; }
        if let Some(v) = map.get("rate_limit_hits") { self.rate_limit_hits = *v; }
    }

    /// Estimate cost in cents based on usage
    pub fn estimate_cost_cents(&self) -> i64 {
        let message_cost = (self.messages_sent + self.messages_received) / 1000;
        let byte_cost = (self.total_bytes_sent + self.total_bytes_received) / 1_000_000;
        message_cost + byte_cost
    }
}

// =============================================================================
// Flusher Metrics (Shared)
// =============================================================================

/// Metrics for tracking flusher performance
#[derive(Debug, Default)]
pub struct FlusherMetrics {
    pub keys_processed: std::sync::atomic::AtomicU64,
    pub batches_processed: std::sync::atomic::AtomicU64,
    pub total_flush_duration_ms: std::sync::atomic::AtomicU64,
    pub errors: std::sync::atomic::AtomicU64,
}

impl FlusherMetrics {
    pub fn record_error(&self) {
        self.errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Metrics for tracking client flusher performance
#[derive(Debug, Default)]
pub struct ClientFlusherMetrics {
    pub keys_processed: std::sync::atomic::AtomicU64,
    pub batches_processed: std::sync::atomic::AtomicU64,
    pub total_flush_duration_ms: std::sync::atomic::AtomicU64,
    pub errors: std::sync::atomic::AtomicU64,
}

impl ClientFlusherMetrics {
    pub fn record_error(&self) {
        self.errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
