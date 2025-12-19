// =============================================================================
// Google OAuth 2.0 Service
// =============================================================================
//!
//! This module implements Google OAuth 2.0 authentication flow for the Vaultless API.
//!
//! ## OAuth Flow Overview:
//! 1. User clicks "Login with Google" → GET /auth/google
//! 2. Server generates state token, redirects to Google consent screen
//! 3. User authenticates with Google
//! 4. Google redirects back to /auth/google/callback with authorization code
//! 5. Server exchanges code for tokens, fetches user profile
//! 6. Server creates/links user account, issues JWT session token
//!
//! ## Security Considerations:
//! - State token prevents CSRF attacks
//! - Client secret is NEVER exposed to frontend
//! - Tokens are validated server-side only
//! - State tokens are stored in Redis with short TTL

use crate::middleware::error::ApiError;
use deadpool_redis::Pool as RedisPool;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// =============================================================================
// GOOGLE OAUTH CONSTANTS
// =============================================================================

/// Google's OAuth 2.0 authorization endpoint
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Google's OAuth 2.0 token exchange endpoint
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Google's user info endpoint
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

/// OAuth state token TTL in Redis (5 minutes)
const STATE_TOKEN_TTL_SECS: u64 = 300;

/// Redis key prefix for OAuth state tokens
const STATE_KEY_PREFIX: &str = "oauth_state:";

// =============================================================================
// DATA STRUCTURES
// =============================================================================

/// Google OAuth token response from token exchange
#[derive(Debug, Deserialize)]
pub struct GoogleTokenResponse {
    /// OAuth access token for API calls
    pub access_token: String,
    /// Token type (usually "Bearer")
    pub token_type: String,
    /// Token expiry in seconds
    pub expires_in: i64,
    /// Refresh token (only on first authorization with offline access)
    pub refresh_token: Option<String>,
    /// Scopes granted
    pub scope: Option<String>,
    /// ID token (JWT) containing user claims
    pub id_token: Option<String>,
}

/// Google user profile information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleUserInfo {
    /// Google's unique user identifier
    pub id: String,
    /// User's email address
    pub email: String,
    /// Whether the email is verified by Google
    pub verified_email: bool,
    /// User's display name
    pub name: Option<String>,
    /// User's given (first) name
    pub given_name: Option<String>,
    /// User's family (last) name
    pub family_name: Option<String>,
    /// URL to user's profile picture
    pub picture: Option<String>,
    /// User's locale/language preference
    pub locale: Option<String>,
}

/// OAuth state data stored in Redis
#[derive(Debug, Serialize, Deserialize)]
struct OAuthState {
    /// Random state token for CSRF protection
    pub state: String,
    /// Original redirect URL after successful auth (optional)
    pub redirect_after: Option<String>,
    /// Timestamp when state was created
    pub created_at: i64,
}

// =============================================================================
// GOOGLE OAUTH SERVICE
// =============================================================================

/// Service for handling Google OAuth 2.0 authentication
#[derive(Clone)]
pub struct GoogleOAuthService {
    /// HTTP client for making requests to Google APIs
    http_client: Client,
    /// Google OAuth Client ID
    client_id: String,
    /// Google OAuth Client Secret
    client_secret: String,
    /// Redirect URI (must match Google Console configuration)
    redirect_uri: String,
    /// Redis pool for storing state tokens
    redis_pool: Arc<RedisPool>,
}

impl GoogleOAuthService {
    /// Create a new GoogleOAuthService instance
    ///
    /// # Arguments
    /// * `client_id` - Google OAuth Client ID from Google Cloud Console
    /// * `client_secret` - Google OAuth Client Secret (NEVER expose to frontend)
    /// * `redirect_uri` - Must exactly match URI configured in Google Console
    /// * `redis_pool` - Redis connection pool for state token storage
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        redis_pool: Arc<RedisPool>,
    ) -> Self {
        Self {
            http_client: Client::new(),
            client_id,
            client_secret,
            redirect_uri,
            redis_pool,
        }
    }

    // =========================================================================
    // STEP 1: Generate Authorization URL
    // =========================================================================

    /// Generate Google OAuth authorization URL with CSRF state token
    ///
    /// This creates a secure authorization URL that redirects users to Google's
    /// consent screen. A cryptographically random state token is generated and
    /// stored in Redis for CSRF protection.
    ///
    /// # Arguments
    /// * `redirect_after` - Optional URL to redirect to after successful auth
    ///
    /// # Returns
    /// * `Ok((auth_url, state))` - The authorization URL and state token
    /// * `Err(ApiError)` - If state storage fails
    ///
    /// # Security
    /// - State token is cryptographically random (UUID v4)
    /// - State is stored in Redis with 5-minute TTL
    /// - State must be validated in callback to prevent CSRF
    pub async fn generate_auth_url(
        &self,
        redirect_after: Option<String>,
    ) -> Result<(String, String), ApiError> {
        // Generate cryptographically random state token for CSRF protection
        let state = Uuid::new_v4().to_string();

        // Store state in Redis with TTL for later validation
        self.store_state(&state, redirect_after).await?;

        // Build OAuth authorization URL with required parameters
        // Scopes requested:
        // - openid: Required for OIDC
        // - email: Access to user's email
        // - profile: Access to basic profile info (name, picture)
        let auth_url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
            GOOGLE_AUTH_URL,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode("openid email profile"),
            urlencoding::encode(&state),
        );

        tracing::debug!(
            state = %state,
            "Generated Google OAuth authorization URL"
        );

        Ok((auth_url, state))
    }

    // =========================================================================
    // STEP 2: Validate State Token (CSRF Protection)
    // =========================================================================

    /// Validate OAuth state token from callback
    ///
    /// This is a critical security check that prevents CSRF attacks.
    /// The state token received in the callback must match one we generated.
    ///
    /// # Arguments
    /// * `state` - State token received in OAuth callback
    ///
    /// # Returns
    /// * `Ok(redirect_after)` - Optional redirect URL if state is valid
    /// * `Err(ApiError)` - If state is invalid or expired (CSRF attempt)
    ///
    /// # Security
    /// - State is deleted from Redis after validation (one-time use)
    /// - Invalid state = potential CSRF attack, reject immediately
    pub async fn validate_state(&self, state: &str) -> Result<Option<String>, ApiError> {
        let key = format!("{}{}", STATE_KEY_PREFIX, state);

        // Get and delete state atomically (one-time use)
        let mut conn = self.redis_pool.get().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to get Redis connection");
            ApiError::internal_server_error("Authentication service unavailable")
        })?;

        let state_data: Option<String> = deadpool_redis::redis::cmd("GETDEL")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to validate OAuth state");
                ApiError::internal_server_error("Failed to validate authentication state")
            })?;

        match state_data {
            Some(data) => {
                let oauth_state: OAuthState = serde_json::from_str(&data).map_err(|e| {
                    tracing::error!(error = %e, "Failed to parse OAuth state");
                    ApiError::internal_server_error("Invalid authentication state")
                })?;

                tracing::debug!(state = %state, "OAuth state validated successfully");
                Ok(oauth_state.redirect_after)
            }
            None => {
                // State not found = invalid or expired = potential CSRF attack
                tracing::warn!(state = %state, "Invalid or expired OAuth state - possible CSRF attempt");
                Err(ApiError::bad_request("Invalid or expired authentication state")
                    .with_code("INVALID_OAUTH_STATE"))
            }
        }
    }

    // =========================================================================
    // STEP 3: Exchange Authorization Code for Tokens
    // =========================================================================

    /// Exchange authorization code for access tokens
    ///
    /// After user consents, Google redirects back with an authorization code.
    /// This code is exchanged server-side for access/refresh tokens.
    ///
    /// # Arguments
    /// * `code` - Authorization code from Google callback
    ///
    /// # Returns
    /// * `Ok(GoogleTokenResponse)` - Contains access_token, refresh_token, etc.
    /// * `Err(ApiError)` - If token exchange fails
    ///
    /// # Security
    /// - Client secret is sent server-side only (NEVER to frontend)
    /// - Code can only be used once
    /// - HTTPS is enforced by Google
    pub async fn exchange_code(&self, code: &str) -> Result<GoogleTokenResponse, ApiError> {
        tracing::debug!("Exchanging authorization code for tokens");

        // Prepare token exchange request
        // This is a server-to-server call with client secret
        let params = [
            ("code", code),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("redirect_uri", &self.redirect_uri),
            ("grant_type", "authorization_code"),
        ];

        let response = self
            .http_client
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to exchange OAuth code");
                ApiError::internal_server_error("Failed to authenticate with Google")
                    .with_code("GOOGLE_TOKEN_EXCHANGE_FAILED")
            })?;

        // Check for error response from Google
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            tracing::error!(
                status = %status,
                error = %error_body,
                "Google token exchange failed"
            );
            return Err(ApiError::bad_request("Google authentication failed")
                .with_code("GOOGLE_AUTH_FAILED"));
        }

        // Parse successful token response
        let token_response: GoogleTokenResponse = response.json().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to parse Google token response");
            ApiError::internal_server_error("Invalid response from Google")
        })?;

        tracing::info!("Successfully exchanged code for Google tokens");
        Ok(token_response)
    }

    // =========================================================================
    // STEP 4: Fetch User Profile from Google
    // =========================================================================

    /// Fetch user profile information from Google
    ///
    /// Uses the access token to retrieve the authenticated user's profile
    /// including email, name, and profile picture.
    ///
    /// # Arguments
    /// * `access_token` - Valid Google OAuth access token
    ///
    /// # Returns
    /// * `Ok(GoogleUserInfo)` - User's Google profile information
    /// * `Err(ApiError)` - If profile fetch fails or token is invalid
    pub async fn get_user_info(&self, access_token: &str) -> Result<GoogleUserInfo, ApiError> {
        tracing::debug!("Fetching Google user info");

        let response = self
            .http_client
            .get(GOOGLE_USERINFO_URL)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to fetch Google user info");
                ApiError::internal_server_error("Failed to fetch user profile from Google")
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            tracing::error!(
                status = %status,
                error = %error_body,
                "Google userinfo request failed"
            );
            return Err(ApiError::unauthorized("Invalid Google access token")
                .with_code("GOOGLE_TOKEN_INVALID"));
        }

        let user_info: GoogleUserInfo = response.json().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to parse Google user info");
            ApiError::internal_server_error("Invalid user profile from Google")
        })?;

        tracing::info!(
            google_id = %user_info.id,
            email = %user_info.email,
            "Successfully fetched Google user info"
        );

        Ok(user_info)
    }

    // =========================================================================
    // HELPER: Store State Token in Redis
    // =========================================================================

    /// Store OAuth state token in Redis
    async fn store_state(
        &self,
        state: &str,
        redirect_after: Option<String>,
    ) -> Result<(), ApiError> {
        let key = format!("{}{}", STATE_KEY_PREFIX, state);

        let oauth_state = OAuthState {
            state: state.to_string(),
            redirect_after,
            created_at: chrono::Utc::now().timestamp(),
        };

        let state_json = serde_json::to_string(&oauth_state).map_err(|e| {
            tracing::error!(error = %e, "Failed to serialize OAuth state");
            ApiError::internal_server_error("Failed to create authentication state")
        })?;

        let mut conn = self.redis_pool.get().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to get Redis connection");
            ApiError::internal_server_error("Authentication service unavailable")
        })?;

        // Store with TTL (5 minutes)
        deadpool_redis::redis::cmd("SETEX")
            .arg(&key)
            .arg(STATE_TOKEN_TTL_SECS)
            .arg(&state_json)
            .query_async::<()>(&mut *conn)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to store OAuth state");
                ApiError::internal_server_error("Failed to initiate authentication")
            })?;

        tracing::debug!(state = %state, "Stored OAuth state in Redis");
        Ok(())
    }

    // =========================================================================
    // COMPLETE FLOW: Convenience method for full OAuth callback handling
    // =========================================================================

    /// Complete OAuth callback handling (validate state, exchange code, get user)
    ///
    /// This is a convenience method that performs the entire callback flow:
    /// 1. Validates the state token (CSRF protection)
    /// 2. Exchanges the authorization code for tokens
    /// 3. Fetches the user's Google profile
    ///
    /// # Arguments
    /// * `code` - Authorization code from Google callback
    /// * `state` - State token from Google callback
    ///
    /// # Returns
    /// * `Ok((GoogleUserInfo, GoogleTokenResponse, redirect_after))` - User info and tokens
    /// * `Err(ApiError)` - If any step fails
    pub async fn handle_callback(
        &self,
        code: &str,
        state: &str,
    ) -> Result<(GoogleUserInfo, GoogleTokenResponse, Option<String>), ApiError> {
        // Step 1: Validate state (CSRF protection)
        let redirect_after = self.validate_state(state).await?;

        // Step 2: Exchange code for tokens
        let tokens = self.exchange_code(code).await?;

        // Step 3: Get user info
        let user_info = self.get_user_info(&tokens.access_token).await?;

        Ok((user_info, tokens, redirect_after))
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_google_userinfo_deserialize() {
        let json = r#"{
            "id": "123456789",
            "email": "test@example.com",
            "verified_email": true,
            "name": "Test User",
            "given_name": "Test",
            "family_name": "User",
            "picture": "https://example.com/photo.jpg",
            "locale": "en"
        }"#;

        let user_info: GoogleUserInfo = serde_json::from_str(json).unwrap();
        assert_eq!(user_info.id, "123456789");
        assert_eq!(user_info.email, "test@example.com");
        assert!(user_info.verified_email);
        assert_eq!(user_info.name, Some("Test User".to_string()));
    }

    #[test]
    fn test_google_token_response_deserialize() {
        let json = r#"{
            "access_token": "ya29.test_token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "1//test_refresh",
            "scope": "openid email profile"
        }"#;

        let token_response: GoogleTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(token_response.access_token, "ya29.test_token");
        assert_eq!(token_response.token_type, "Bearer");
        assert_eq!(token_response.expires_in, 3600);
        assert_eq!(token_response.refresh_token, Some("1//test_refresh".to_string()));
    }
}
