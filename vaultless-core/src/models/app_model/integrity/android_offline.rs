use super::types::*;
use crate::error::{Result, VaultlessError};
use crate::models::app_model::integrity::dto::AndroidIntegrityConfig;
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use once_cell::sync::OnceCell;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::Value as jsonValue;
use std::sync::Arc;
use std::time::Duration;
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
    #[serde(rename = "versionCode", alias = "apkVersionCode")]
    pub version_code: Option<i64>,
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

// Custom deserializer for timestamp
fn deserialize_timestamp<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Deserialize};
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| de::Error::custom("Invalid timestamp")),
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
// JWKS CACHING
// =============================================================================

static GOOGLE_JWKS_CACHE: OnceCell<Arc<RwLock<Option<CachedJwks>>>> = OnceCell::new();
const JWKS_CACHE_TTL_HOURS: i64 = 24;

async fn get_google_jwks_cached(client: &HttpClient) -> Result<Jwks> {
    let cache = GOOGLE_JWKS_CACHE
        .get_or_init(|| Arc::new(RwLock::new(None)))
        .clone();

    {
        let r = cache.read().await;
        if let Some(ref cached) = *r {
            if Utc::now()
                .signed_duration_since(cached.cached_at)
                .num_hours()
                < JWKS_CACHE_TTL_HOURS
            {
                return Ok(cached.jwks.clone());
            }
        }
    }

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
// ATTESTATION RESULT BUILDER (updated with trust_score_percent)
// =============================================================================

struct AttestationResultBuilder {
    is_valid: bool,
    platform: Platform,
    extra: jsonValue,

    device_trusted: bool,
    verdict: Option<String>,
    error: Option<String>,
    warnings: Vec<String>,
    trust_score_percent: u8,
}

impl AttestationResultBuilder {
    fn new(platform: Platform) -> Self {
        Self {
            is_valid: false,
            platform,
            extra: jsonValue::Null,
            device_trusted: false,
            verdict: None,
            error: None,
            warnings: Vec::new(),
            trust_score_percent: 0,
        }
    }

    fn valid(mut self) -> Self {
        self.is_valid = true;
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

    fn trust_score(mut self, score: u8) -> Self {
        self.trust_score_percent = score;
        self
    }

    fn extra(mut self, value: jsonValue) -> Self {
        self.extra = value;
        self
    }

    fn build(self) -> AttestationResult {
        AttestationResult {
            is_valid: self.is_valid,
            device_trusted: self.device_trusted,
            trust_score_percent: self.trust_score_percent,
            verdict: self.verdict,
            error: self.error,
            warnings: if self.warnings.is_empty() {
                None
            } else {
                Some(self.warnings)
            },
            verified_at: Utc::now(),
            extra: self.extra,
        }
    }
}

// =============================================================================
// VERIFICATION FUNCTIONS
// =============================================================================

fn verify_timestamp(timestamp_ms: i64, max_age_seconds: u64) -> std::result::Result<(), String> {
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
        "UNRECOGNIZED_VERSION" if !reject_unrecognized => Ok(Some(
            "App version not recognized (testing/staged rollout)".to_string(),
        )),
        "UNRECOGNIZED_VERSION" => Err("App version not recognized by Play Store".to_string()),
        _ => Err(format!("App not recognized: {}", verdict)),
    }
}

fn check_device_integrity(
    verdicts: &[String],
    reject_untrusted: bool,
) -> std::result::Result<bool, String> {
    let trusted = verdicts.iter().any(|v| {
        matches!(
            v.as_str(),
            "MEETS_DEVICE_INTEGRITY" | "MEETS_BASIC_INTEGRITY"
        )
    });

    if trusted {
        Ok(true)
    } else {
        let warning = format!("Device integrity failed: {:?}", verdicts);
        if reject_untrusted {
            Err(warning)
        } else {
            Ok(false)
        }
    }
}

/// Verify Android Play Integrity attestation
pub async fn verify_android_attestation_offline(
    token: &str,
    config: &AndroidIntegrityConfig,
) -> Result<AttestationResult> {
    use serde_json::{Value as JsonValue, json};

    let mut warnings = Vec::new();
    let mut score: u8 = 0;

    let http_client = HttpClient::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| VaultlessError::Internal(format!("HTTP client error: {}", e)))?;

    // Header decode = base trust
    let header = decode_header(token)
        .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid JWS header: {}", e)))?;
    score += 20;

    let kid = header.kid.ok_or_else(|| {
        VaultlessError::IntegrityCheckFailed("JWS missing 'kid' header".to_string())
    })?;

    // Fetch JWKS
    let jwks = get_google_jwks_cached(&http_client).await?;

    let jwk = jwks.keys.iter().find(|k| k.kid == kid).ok_or_else(|| {
        VaultlessError::IntegrityCheckFailed(format!("No JWK found for kid '{}'", kid))
    })?;

    let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid JWK RSA components: {}", e))
    })?;

    // Signature validation
    let mut validation = Validation::new(header.alg);
    validation.validate_exp = false;
    validation.validate_nbf = false;

    let token_data =
        decode::<PlayIntegrityClaims>(token, &decoding_key, &validation).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!(
                "JWS signature verification failed: {}",
                e
            ))
        })?;

    score += 20;

    let claims = token_data.claims;

    // Build AndroidData (your domain struct)
    let android_data = AndroidData {
        package_name: claims.app_integrity.package_name.clone(),
        certificate_sha256: claims
            .app_integrity
            .certificate_sha256_digest
            .get(0)
            .cloned()
            .unwrap_or_default(),
        attestation_token: token.to_string(),
    };

    // Build "extra" JSON for storage
    let extra_json = json!({
        "pkg_name": claims.app_integrity.package_name,
        "cert_sha256_digest": claims.app_integrity.certificate_sha256_digest,
        "app_reg_verdict": claims.app_integrity.app_recognition_verdict,
        "device_reg_verdict": claims.device_integrity.device_recognition_verdict,
        "timestamp_ms": claims.request_details.timestamp_millis,
        "account_details": claims.account_details,
    });

    // Build the attestation result
    let builder = AttestationResultBuilder::new(Platform::Android).extra(extra_json);

    // Timestamp verification
    if let Err(e) = verify_timestamp(
        claims.request_details.timestamp_millis,
        config.max_token_age_seconds.unwrap_or(300),
    ) {
        return Ok(builder
            .trust_score(score)
            .verdict("TIMESTAMP_ERROR".to_string())
            .error(e)
            .build());
    } else {
        score += 15;
    }

    // Check allowed package name
    if !config.allowed_package_names.is_empty()
        && config
            .allowed_package_names
            .iter()
            .any(|p| p == &claims.app_integrity.package_name)
    {
        score += 15;
    }

    // Certificate SHA256
    let claims_hashes = &claims.app_integrity.certificate_sha256_digest;
    let allowed_hashes = &config.allowed_certificate_sha256;

    if !allowed_hashes.is_empty() {
        if allowed_hashes
            .iter()
            .any(|expected_hash| verify_certificate_hash(claims_hashes, expected_hash).is_ok())
        {
            score += 15;
        }
    }

    // App recognition
    if check_app_recognition(
        &claims.app_integrity.app_recognition_verdict,
        config.reject_unrecognized_version.unwrap_or(false),
    )
    .is_ok()
    {
        score += 10;
    }

    // Device integrity
    let device_trusted = check_device_integrity(
        &claims.device_integrity.device_recognition_verdict,
        config.reject_untrusted_device.unwrap_or(false),
    )
    .unwrap_or(false);

    if device_trusted {
        score += 15;
    }

    // Licensing
    if config.reject_unlicensed_app.unwrap_or(false) {
        if let Some(account) = &claims.account_details {
            if let Some(verdict) = &account.app_licensing_verdict {
                if verdict == "LICENSED" {
                    score += 10;
                }
            }
        }
    } else {
        score += 10;
    }

    // Cap score at 100
    if score > 100 {
        score = 100;
    }

    Ok(builder
        .valid()
        .device_trusted(device_trusted)
        .trust_score(score)
        .warnings(warnings)
        .build())
}
