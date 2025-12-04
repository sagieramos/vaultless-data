use super::types::*;
use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Utc};
use std::time::Duration;

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use once_cell::sync::OnceCell;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// =============================================================================
// TYPES
// =============================================================================

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlayIntegrityClaims {
    #[serde(rename = "requestDetails")]
    request_details: RequestDetails,
    #[serde(rename = "appIntegrity")]
    app_integrity: AppIntegrity,
    #[serde(rename = "deviceIntegrity")]
    device_integrity: DeviceIntegrity,
    #[serde(rename = "accountDetails")]
    account_details: Option<AccountDetails>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RequestDetails {
    #[serde(rename = "timestampMillis")]
    #[serde(deserialize_with = "deserialize_timestamp")]
    timestamp_millis: i64,
    nonce: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AppIntegrity {
    #[serde(rename = "packageName")]
    package_name: String,
    #[serde(rename = "certificateSha256Digest")]
    certificate_sha256_digest: Vec<String>,
    #[serde(rename = "appRecognitionVerdict")]
    app_recognition_verdict: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct DeviceIntegrity {
    #[serde(rename = "deviceRecognitionVerdict")]
    device_recognition_verdict: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AccountDetails {
    #[serde(rename = "appLicensingVerdict")]
    app_licensing_verdict: Option<String>,
}

// Custom deserializer for timestamp that handles both string and number
fn deserialize_timestamp<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Deserialize};
    use serde_json::Value;
    
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Number(n) => n.as_i64().ok_or_else(|| de::Error::custom("Invalid timestamp")),
        Value::String(s) => s.parse::<i64>().map_err(de::Error::custom),
        _ => Err(de::Error::custom("Timestamp must be number or string")),
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Jwks {
    keys: Vec<JwkKey>,
}

#[derive(Debug, Clone, Deserialize)]
struct JwkKey {
    kid: String,
    n: String,
    e: String,
}

struct CachedJwks {
    jwks: Jwks,
    cached_at: DateTime<Utc>,
}

// =============================================================================
// JWKS CACHING WITH TTL
// =============================================================================

static GOOGLE_JWKS_CACHE: OnceCell<Arc<RwLock<Option<CachedJwks>>>> = OnceCell::new();
const JWKS_CACHE_TTL_HOURS: i64 = 24;

async fn get_google_jwks_cached(client: &HttpClient) -> Result<Jwks> {
    let cache = GOOGLE_JWKS_CACHE
        .get_or_init(|| Arc::new(RwLock::new(None)))
        .clone();

    // Check cache with read lock
    {
        let r = cache.read().await;
        if let Some(ref cached) = *r {
            let age = Utc::now().signed_duration_since(cached.cached_at);
            if age.num_hours() < JWKS_CACHE_TTL_HOURS {
                return Ok(cached.jwks.clone());
            }
        }
    }

    // Fetch fresh JWKS
    let url = "https://www.gstatic.com/play-integrity/attestation-keys.json";
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Failed to fetch JWKS: {}", e))
        })?;

    if !resp.status().is_success() {
        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "Failed to fetch JWKS: HTTP {}",
            resp.status()
        )));
    }

    let jwks: Jwks = resp
        .json()
        .await
        .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid JWKS JSON: {}", e)))?;

    // Update cache
    {
        let mut w = cache.write().await;
        *w = Some(CachedJwks {
            jwks: jwks.clone(),
            cached_at: Utc::now(),
        });
    }

    Ok(jwks)
}

// =============================================================================
// ATTESTATION RESULT BUILDER
// =============================================================================

struct AttestationResultBuilder {
    is_valid: bool,
    certificate_hash: String,
    bundle_id: String,
    device_trusted: bool,
    verdict: Option<String>,
    error: Option<String>,
    warnings: Vec<String>,
}

impl AttestationResultBuilder {
    fn new(bundle_id: String) -> Self {
        Self {
            is_valid: false,
            certificate_hash: String::new(),
            bundle_id,
            device_trusted: false,
            verdict: None,
            error: None,
            warnings: Vec::new(),
        }
    }

    fn valid(mut self) -> Self {
        self.is_valid = true;
        self
    }

    fn certificate_hash(mut self, hash: String) -> Self {
        self.certificate_hash = hash;
        self
    }

    fn device_trusted(mut self, trusted: bool) -> Self {
        self.device_trusted = trusted;
        self
    }

    fn verdict(mut self, verdict: String) -> Self {
        self.verdict = Some(verdict);
        self
    }

    fn error(mut self, error: String) -> Self {
        self.error = Some(error);
        self
    }

    fn warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    fn build(self) -> AttestationResult {
        AttestationResult {
            is_valid: self.is_valid,
            certificate_hash: self.certificate_hash,
            bundle_id: self.bundle_id,
            platform: Platform::Android,
            device_trusted: self.device_trusted,
            verdict: self.verdict,
            error: self.error,
            warnings: if self.warnings.is_empty() {
                None
            } else {
                Some(self.warnings)
            },
            verified_at: Utc::now(),
        }
    }
}

// =============================================================================
// VERIFICATION FUNCTIONS
// =============================================================================

fn verify_timestamp(
    timestamp_ms: i64,
    max_age_seconds: u64,
) -> std::result::Result<(), String> {
    let now_ms = Utc::now().timestamp_millis();
    let age_ms = now_ms - timestamp_ms;
    let max_age_ms = (max_age_seconds * 1000) as i64;

    if age_ms > max_age_ms {
        return Err(format!(
            "Token timestamp is {}ms old (max allowed: {}ms)",
            age_ms, max_age_ms
        ));
    }
    
    if age_ms < -5_000 {
        return Err("Token timestamp is in the future (possible clock skew attack)".to_string());
    }
    
    Ok(())
}

fn verify_nonce(actual: &str, expected: &str) -> std::result::Result<(), String> {
    if actual != expected {
        Err("Nonce mismatch (possible replay attack)".to_string())
    } else {
        Ok(())
    }
}

fn verify_package_name(actual: &str, expected: &str) -> std::result::Result<(), String> {
    if actual != expected {
        Err(format!("Expected package '{}', got '{}'", expected, actual))
    } else {
        Ok(())
    }
}

fn verify_certificate_hash(
    certificate_digests: &[String],
    expected_hash: &str,
) -> std::result::Result<(), String> {
    let cert_match = certificate_digests
        .iter()
        .any(|h| h.eq_ignore_ascii_case(expected_hash));
    
    if !cert_match {
        Err("Certificate hash mismatch".to_string())
    } else {
        Ok(())
    }
}

fn check_app_recognition(
    verdict: &str,
    reject_unrecognized: bool,
) -> std::result::Result<Option<String>, String> {
    match verdict {
        "PLAY_RECOGNIZED" => Ok(None),
        "UNRECOGNIZED_VERSION" if !reject_unrecognized => {
            Ok(Some("App version not recognized (testing/staged rollout)".to_string()))
        }
        "UNRECOGNIZED_VERSION" => {
            Err("App version not recognized by Play Store".to_string())
        }
        _ => Err(format!("App not recognized: {}", verdict)),
    }
}

fn check_device_integrity(
    verdicts: &[String],
    reject_untrusted: bool,
) -> (bool, Option<String>) {
    let trusted = verdicts.iter().any(|v| {
        matches!(
            v.as_str(),
            "MEETS_DEVICE_INTEGRITY" | "MEETS_BASIC_INTEGRITY"
        )
    });

    if !trusted {
        let warning = format!("Device integrity failed: {:?}", verdicts);
        if reject_untrusted {
            (false, Some(warning))
        } else {
            (false, Some(warning))
        }
    } else {
        (true, None)
    }
}

/// Verify Android Play Integrity attestation 
pub async fn verify_android_attestation_offline(
    token: &str,
    expected_package_name: &str,
    expected_cert_hash: &str,
    max_token_age_seconds: u64,
    reject_unrecognized_version: bool,
    reject_untrusted_device: bool,
) -> Result<AttestationResult> {
    let mut warnings = Vec::new();

    // Build HTTP client
    let http_client = HttpClient::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| VaultlessError::Internal(format!("HTTP client error: {}", e)))?;

    // Parse token header
    let header = decode_header(token)
        .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid JWS header: {}", e)))?;

    let kid = header
        .kid
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("JWS missing 'kid' header".to_string()))?;

    // Fetch JWKS
    let jwks = get_google_jwks_cached(&http_client).await?;

    // Find matching key
    let jwk = jwks
        .keys
        .iter()
        .find(|k| k.kid == kid)
        .ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(format!("No JWK found for kid '{}'", kid))
        })?;

    // Create decoding key
    let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid JWK RSA components: {}", e))
    })?;

    let mut validation = Validation::new(header.alg);
    validation.validate_exp = false;
    validation.validate_nbf = false;

    // Decode and verify signature
    let token_data = decode::<PlayIntegrityClaims>(token, &decoding_key, &validation)
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("JWS signature verification failed: {}", e))
        })?;

    let claims = token_data.claims;
    let package_name = claims.app_integrity.package_name.clone();

    let mut builder = AttestationResultBuilder::new(package_name.clone());

    // Verify timestamp
    if let Err(e) = verify_timestamp(claims.request_details.timestamp_millis, max_token_age_seconds) {
        return Ok(builder.verdict("TIMESTAMP_ERROR".to_string()).error(e).build());
    }

    // NOTE: Nonce/challenge verification is handled externally by verify_and_consume_challenge()
    // before this function is called. We don't check claims.request_details.nonce here.

    // Verify package name
    if let Err(e) = verify_package_name(&package_name, expected_package_name) {
        return Ok(builder.verdict("PACKAGE_NAME_MISMATCH".to_string()).error(e).build());
    }

    // Check app recognition
    match check_app_recognition(&claims.app_integrity.app_recognition_verdict, reject_unrecognized_version) {
        Err(e) => {
            return Ok(builder
                .verdict(claims.app_integrity.app_recognition_verdict.clone())
                .error(e)
                .build());
        }
        Ok(Some(warning)) => warnings.push(warning),
        Ok(None) => {}
    }

    // Verify certificate hash
    let cert_hash_str = claims.app_integrity.certificate_sha256_digest.join(",");
    if let Err(e) = verify_certificate_hash(
        &claims.app_integrity.certificate_sha256_digest,
        expected_cert_hash,
    ) {
        return Ok(builder
            .certificate_hash(cert_hash_str)
            .verdict(claims.app_integrity.app_recognition_verdict.clone())
            .error(e)
            .build());
    }

    // Check device integrity
    let (device_trusted, device_warning) = check_device_integrity(
        &claims.device_integrity.device_recognition_verdict,
        reject_untrusted_device,
    );

    if let Some(warning) = device_warning {
        if reject_untrusted_device {
            return Ok(builder
                .certificate_hash(cert_hash_str)
                .verdict(format!("DEVICE:{:?}", claims.device_integrity.device_recognition_verdict))
                .error(warning)
                .build());
        } else {
            warnings.push(warning);
        }
    }

    // Check licensing (informational)
    if let Some(account) = claims.account_details {
        if let Some(licensing) = account.app_licensing_verdict {
            if licensing != "LICENSED" {
                warnings.push(format!("App licensing status: {}", licensing));
            }
        }
    }

    // Success!
    Ok(builder
        .valid()
        .certificate_hash(cert_hash_str)
        .device_trusted(device_trusted)
        .verdict(format!(
            "APP:{}, DEVICE:{:?}",
            claims.app_integrity.app_recognition_verdict,
            claims.device_integrity.device_recognition_verdict
        ))
        .warnings(warnings)
        .build())
}
