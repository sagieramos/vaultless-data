use super::error::ApiError;
use axum::http::{HeaderMap, HeaderName};
use hyper::header;

pub fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    // First try Authorization header
    if let Some(auth_header) = headers.get(header::AUTHORIZATION) {
        let auth_str = auth_header
            .to_str()
            .map_err(|_| ApiError::unauthorized("Invalid Authorization header"))?;

        let prefix = "Bearer ";
        if !auth_str.starts_with(prefix) {
            return Err(ApiError::unauthorized(
                "Invalid Authorization format. Expected: Bearer <token>",
            ));
        }

        let token = auth_str.trim_start_matches(prefix).trim();

        if token.is_empty() {
            return Err(ApiError::unauthorized("Empty bearer token").with_code("EMPTY_TOKEN"));
        }

        return Ok(token);
    }

    // Fallback to access_token cookie if present
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            // cookie_str like "key1=val1; key2=val2"
            for part in cookie_str.split(';') {
                let kv = part.trim();
                if let Some(val) = kv.strip_prefix("access_token=") {
                    let token = val.trim();
                    if !token.is_empty() {
                        return Ok(token);
                    } else {
                        return Err(ApiError::unauthorized("Empty bearer token").with_code("EMPTY_TOKEN"));
                    }
                }
            }
        }
    }

    Err(ApiError::unauthorized("Missing Authorization header").with_code("MISSING_AUTH_HEADER"))
}

pub fn extract_api_key(headers: &HeaderMap) -> Result<&str, ApiError> {
    static API_KEY_HEADER: HeaderName = HeaderName::from_static("x-api-key");

    let api_key_header = headers.get(&API_KEY_HEADER).ok_or_else(|| {
        ApiError::unauthorized("Missing API key header").with_code("MISSING_API_KEY")
    })?;

    let api_key = api_key_header
        .to_str()
        .map_err(|_| ApiError::unauthorized("Invalid API key header encoding"))?
        .trim();

    if api_key.is_empty() {
        return Err(ApiError::unauthorized("Empty API key").with_code("EMPTY_API_KEY"));
    }

    Ok(api_key)
}
