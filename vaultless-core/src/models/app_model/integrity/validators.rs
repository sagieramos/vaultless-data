use crate::error::VaultlessError;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;

// Origin  Browser validators
lazy_static! {
    static ref ORIGIN_RE: Regex = Regex::new(r"^https://[a-zA-Z0-9.-]+(:\d{1,5})?$").unwrap();
}

const WHITELISTED_DOMAINS: &[&str] = &["example.com", "*.example.org", "myapp.io"];

/// Validate a single origin URL
pub fn validate_origin(origin: &str) -> Result<(), VaultlessError> {
    if ORIGIN_RE.is_match(origin) {
        Ok(())
    } else {
        Err(VaultlessError::IntegrityCheckFailed(format!(
            "Invalid origin '{}'. Must match https://domain[:port]",
            origin
        )))
    }
}

/// Validate a list of origins: max length, no duplicates, subdomain restrictions
pub fn validate_origin_list(origins: &Vec<String>) -> Result<(), VaultlessError> {
    if origins.len() > 20 {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Too many authorized origins (max 20)".into(),
        ));
    }

    let mut seen = HashSet::new();
    for origin in origins {
        // Check duplicates
        if !seen.insert(origin) {
            return Err(VaultlessError::IntegrityCheckFailed(format!(
                "Duplicate origin '{}'",
                origin
            )));
        }

        // Check format
        validate_origin(origin)?;

        // Subdomain restriction: wildcard allowed only at start
        if origin.contains('*') && !origin.starts_with("https://*.") {
            return Err(VaultlessError::IntegrityCheckFailed(format!(
                "Invalid wildcard placement in origin '{}'",
                origin
            )));
        }
    }

    Ok(())
}

/// Check host against whitelist (supports wildcards)
fn is_domain_allowed(host: &str) -> bool {
    for pattern in WHITELISTED_DOMAINS {
        if pattern.starts_with("*.") {
            let domain = &pattern[2..];
            if host.ends_with(domain) && host != domain {
                return true;
            }
        } else if host == *pattern {
            return true;
        }
    }
    false
}

/// Validate optional string length
pub fn validate_optional_string_len(
    value: &String,
    min: usize,
    max: usize,
) -> Result<(), VaultlessError> {
    let len = value.len();
    if len < min || len > max {
        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "String length must be between {} and {}, got {}",
            min, max, len
        )));
    }
    Ok(())
}

// Bundle ID (iOS/Android)
lazy_static! {
    static ref BUNDLE_ID_RE: Regex =
        Regex::new(r"^[A-Za-z][A-Za-z0-9-]*(\.[A-Za-z0-9-]+)+$").unwrap();
}

pub fn validate_bundle_id(id: &str) -> Result<(), VaultlessError> {
    if BUNDLE_ID_RE.is_match(id) {
        Ok(())
    } else {
        Err(VaultlessError::IntegrityCheckFailed(format!(
            "Invalid bundle ID '{}', must follow com.company.app format",
            id
        )))
    }
}

// SHA-256 validator
lazy_static! {
    static ref SHA256_RE: Regex = Regex::new(r"^[A-Fa-f0-9]{64}$").unwrap();
}

pub fn validate_sha256(val: &str) -> Result<(), VaultlessError> {
    if SHA256_RE.is_match(val) {
        Ok(())
    } else {
        Err(VaultlessError::IntegrityCheckFailed(format!(
            "Invalid SHA-256 hash '{}', must be 64 hex characters",
            val
        )))
    }
}

// Version validator
lazy_static! {
    static ref VERSION_RE: Regex =
        Regex::new(r"^(?:\d+)(?:\.\d+){0,2}(?:-[0-9A-Za-z\.-]+)?$").unwrap();
}

pub fn validate_version_format(v: &str) -> Result<(), VaultlessError> {
    if VERSION_RE.is_match(v) {
        Ok(())
    } else {
        Err(VaultlessError::IntegrityCheckFailed(format!(
            "Invalid version format '{}'",
            v
        )))
    }
}

// Device ID (IoT)
lazy_static! {
    static ref DEVICE_ID_RE: Regex = Regex::new(r"^[A-Za-z0-9+/=]{10,64}$").unwrap();
}

pub fn validate_device_id(id: &str) -> Result<(), VaultlessError> {
    if DEVICE_ID_RE.is_match(id) {
        Ok(())
    } else {
        Err(VaultlessError::IntegrityCheckFailed(format!(
            "Invalid device ID '{}'",
            id
        )))
    }
}

// Certificate Authority Name
lazy_static! {
    static ref CA_NAME_RE: Regex = Regex::new(r"^[A-Za-z0-9 ._-]{3,64}$").unwrap();
}

pub fn validate_ca_name(val: &str) -> Result<(), VaultlessError> {
    if CA_NAME_RE.is_match(val) {
        Ok(())
    } else {
        Err(VaultlessError::IntegrityCheckFailed(format!(
            "Invalid CA name '{}'",
            val
        )))
    }
}
