use crate::error::{Result, VaultlessError};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// =============================================================================
// CAPTCHA PROVIDER TYPES
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaptchaProvider {
    #[serde(rename = "turnstile")]
    Turnstile, // Cloudflare Turnstile (recommended)
    #[serde(rename = "hcaptcha")]
    HCaptcha,
    #[serde(rename = "recaptcha")]
    ReCaptcha,
}

impl CaptchaProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            CaptchaProvider::Turnstile => "turnstile",
            CaptchaProvider::HCaptcha => "hcaptcha",
            CaptchaProvider::ReCaptcha => "recaptcha",
        }
    }
}

// =============================================================================
// CLOUDFLARE TURNSTILE
// =============================================================================

#[derive(Debug, Serialize)]
struct TurnstileVerifyRequest {
    secret: String,
    response: String,
    remoteip: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TurnstileVerifyResponse {
    success: bool,
    #[serde(rename = "error-codes")]
    error_codes: Option<Vec<String>>,
    challenge_ts: Option<String>,
    hostname: Option<String>,
}

async fn verify_turnstile(token: &str, secret: &str, ip_address: Option<&str>) -> Result<bool> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| VaultlessError::Internal(format!("HTTP client error: {}", e)))?;

    let request = TurnstileVerifyRequest {
        secret: secret.to_string(),
        response: token.to_string(),
        remoteip: ip_address.map(String::from),
    };

    let response = client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Turnstile API request failed: {}", e))
        })?;

    if !response.status().is_success() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Turnstile API returned error status".into(),
        ));
    }

    let verify_response: TurnstileVerifyResponse = response.json().await.map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid Turnstile response: {}", e))
    })?;

    if !verify_response.success {
        tracing::warn!(
            error_codes = ?verify_response.error_codes,
            "Turnstile verification failed"
        );
        return Ok(false);
    }

    Ok(true)
}

// =============================================================================
// HCAPTCHA
// =============================================================================

#[derive(Debug, Serialize)]
struct HCaptchaVerifyRequest {
    secret: String,
    response: String,
    remoteip: Option<String>,
    sitekey: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HCaptchaVerifyResponse {
    success: bool,
    #[serde(rename = "error-codes")]
    error_codes: Option<Vec<String>>,
    challenge_ts: Option<String>,
    hostname: Option<String>,
}

async fn verify_hcaptcha(
    token: &str,
    secret: &str,
    site_key: Option<&str>,
    ip_address: Option<&str>,
) -> Result<bool> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| VaultlessError::Internal(format!("HTTP client error: {}", e)))?;

    let request = HCaptchaVerifyRequest {
        secret: secret.to_string(),
        response: token.to_string(),
        remoteip: ip_address.map(String::from),
        sitekey: site_key.map(String::from),
    };

    let response = client
        .post("https://hcaptcha.com/siteverify")
        .form(&request)
        .send()
        .await
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("hCaptcha API request failed: {}", e))
        })?;

    if !response.status().is_success() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "hCaptcha API returned error status".into(),
        ));
    }

    let verify_response: HCaptchaVerifyResponse = response.json().await.map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid hCaptcha response: {}", e))
    })?;

    if !verify_response.success {
        tracing::warn!(
            error_codes = ?verify_response.error_codes,
            "hCaptcha verification failed"
        );
        return Ok(false);
    }

    Ok(true)
}

// =============================================================================
// GOOGLE RECAPTCHA
// =============================================================================

#[derive(Debug, Serialize)]
struct ReCaptchaVerifyRequest {
    secret: String,
    response: String,
    remoteip: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReCaptchaVerifyResponse {
    success: bool,
    #[serde(rename = "error-codes")]
    error_codes: Option<Vec<String>>,
    challenge_ts: Option<String>,
    hostname: Option<String>,
    score: Option<f64>, // For reCAPTCHA v3
}

async fn verify_recaptcha(
    token: &str,
    secret: &str,
    ip_address: Option<&str>,
    min_score: f64,
) -> Result<bool> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| VaultlessError::Internal(format!("HTTP client error: {}", e)))?;

    let request = ReCaptchaVerifyRequest {
        secret: secret.to_string(),
        response: token.to_string(),
        remoteip: ip_address.map(String::from),
    };

    let response = client
        .post("https://www.google.com/recaptcha/api/siteverify")
        .form(&request)
        .send()
        .await
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("reCAPTCHA API request failed: {}", e))
        })?;

    if !response.status().is_success() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "reCAPTCHA API returned error status".into(),
        ));
    }

    let verify_response: ReCaptchaVerifyResponse = response.json().await.map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid reCAPTCHA response: {}", e))
    })?;

    if !verify_response.success {
        tracing::warn!(
            error_codes = ?verify_response.error_codes,
            "reCAPTCHA verification failed"
        );
        return Ok(false);
    }

    // Check score for v3 (optional)
    if let Some(score) = verify_response.score
        && score < min_score
    {
        tracing::warn!(
            score = score,
            min_score = min_score,
            "reCAPTCHA score too low"
        );
        return Ok(false);
    }

    Ok(true)
}

// =============================================================================
// UNIFIED CAPTCHA VERIFICATION
// =============================================================================

/// Verify CAPTCHA token using the specified provider
pub async fn verify_captcha(
    provider: CaptchaProvider,
    token: &str,
    secret: &str,
    site_key: Option<&str>,
    ip_address: Option<&str>,
) -> Result<bool> {
    match provider {
        CaptchaProvider::Turnstile => verify_turnstile(token, secret, ip_address).await,
        CaptchaProvider::HCaptcha => verify_hcaptcha(token, secret, site_key, ip_address).await,
        CaptchaProvider::ReCaptcha => {
            verify_recaptcha(token, secret, ip_address, 0.5).await // Default min score 0.5
        }
    }
}

// =============================================================================
// CONFIGURATION VALIDATION
// =============================================================================

/// Validate CAPTCHA configuration
pub fn validate_captcha_config(
    provider: CaptchaProvider,
    site_key: Option<&str>,
    secret_key: Option<&str>,
) -> Result<()> {
    if secret_key.is_none() {
        return Err(VaultlessError::Validation(format!(
            "{} secret key is required",
            provider.as_str()
        )));
    }

    if provider == CaptchaProvider::HCaptcha && site_key.is_none() {
        return Err(VaultlessError::Validation(
            "hCaptcha site key is required".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_captcha_provider_serialization() {
        let provider = CaptchaProvider::Turnstile;
        let json = serde_json::to_string(&provider).unwrap();
        assert_eq!(json, r#""turnstile""#);

        let parsed: CaptchaProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, CaptchaProvider::Turnstile);
    }

    #[test]
    fn test_validate_captcha_config() {
        // Valid Turnstile config
        assert!(validate_captcha_config(CaptchaProvider::Turnstile, None, Some("secret")).is_ok());

        // Invalid - missing secret
        assert!(validate_captcha_config(CaptchaProvider::Turnstile, None, None).is_err());

        // Invalid - hCaptcha needs site key
        assert!(validate_captcha_config(CaptchaProvider::HCaptcha, None, Some("secret")).is_err());

        // Valid hCaptcha
        assert!(
            validate_captcha_config(CaptchaProvider::HCaptcha, Some("site_key"), Some("secret"))
                .is_ok()
        );
    }
}
