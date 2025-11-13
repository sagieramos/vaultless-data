use crate::middleware::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{FromRequestParts, State}, // Note: State is not used here but often imported
    http::{StatusCode, header, request::Parts},
    response::IntoResponse,
};
use vaultless_core::models::app_model::{
    CachedResolvedKeyBundle, KeyGranularity, ValidationError, ValidationFailureType,
};
const CUSTOM_API_KEY_HEADER: &str = "X-API-Key";
// Zero-cost, compile-time concatenated error message
const ERROR_MESSAGE_MISSING: &str = "Required header 'X-API-Key' missing.";

// =============================================================================
// 1. Custom Rejection Type (AuthRejection is kept, but it wraps ApiError)
// =============================================================================

/// Rejection type for the key extractor, converting ValidationErrors into ApiErrors.
pub struct AuthRejection(ApiError);

impl IntoResponse for AuthRejection {
    fn into_response(self) -> axum::response::Response {
        self.0.into_response()
    }
}

/// Converts a ValidationError into an ApiError with the correct HTTP status code.
impl From<ValidationError> for AuthRejection {
    fn from(err: ValidationError) -> Self {
        let (status, code) = match err.type_code {
            ValidationFailureType::NotFound | ValidationFailureType::InvalidKey => {
                (StatusCode::UNAUTHORIZED, "INVALID_KEY")
            }
            ValidationFailureType::Deactivated => (StatusCode::FORBIDDEN, "API_KEY_INACTIVE"),
            ValidationFailureType::Expired => (StatusCode::UNAUTHORIZED, "API_KEY_EXPIRED"),
            ValidationFailureType::QuotaExhausted => (StatusCode::FORBIDDEN, "QUOTA_EXCEEDED"),
            ValidationFailureType::RateLimitHit => {
                (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMIT_EXCEEDED")
            }
            ValidationFailureType::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN_ORIGIN"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        AuthRejection(ApiError::new(status, err.message).with_code(code))
    }
}

// =============================================================================
// 2. Extractor Type (Refactored to use generic state S)
// =============================================================================

/// An Axum Extractor that resolves and validates an API key from the request
/// and provides the validated bundle to the handler.
#[derive(Clone)] // Added derive(Clone) just in case, though usually not needed for Extractors
pub struct ValidatedKeyBundle(pub CachedResolvedKeyBundle);

impl<S> FromRequestParts<S> for ValidatedKeyBundle
where
    // S must be Send + Sync (standard for Axum state)
    S: Send + Sync,
    // We require a way to get AppState from S (i.e., S *is* AppState or contains it)
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = AuthRejection;

    // The function signature is clean and avoids explicit lifetimes
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // --- 0. Get App State ---
        // This is the key step: extracting the AppState from the generic state S
        let app_state: AppState = axum::extract::FromRef::from_ref(state);

        // --- 1. Extract Key (from AUTHORIZATION) and Origin Headers ---

        // ...
        let auth_header = parts
            .headers
            .get(CUSTOM_API_KEY_HEADER)
            .and_then(|h| h.to_str().ok());

        let key_plaintext = match auth_header {
            Some(key) => key.trim(),
            None => {
                // Use the pre-calculated, static error message
                return Err(AuthRejection(
                    ApiError::unauthorized(ERROR_MESSAGE_MISSING)
                        .with_code("MISSING_API_KEY_HEADER"),
                ));
            }
        };

        // Extract the Origin header
        let request_origin = parts
            .headers
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok());

        // Determine granularity based on key prefix (pk_ or sk_)
        let granularity = if key_plaintext.starts_with("pk_") {
            KeyGranularity::Publishable
        } else {
            KeyGranularity::Secret
        };

        // --- 2. Resolution & Hot Validation (Quota, Rate Limits, Expiry) ---
        let validation_result = CachedResolvedKeyBundle::resolve_and_validate(
            app_state.db.as_ref(),        // Use the extracted AppState's DB
            app_state.redis_pool.clone(), // Use the extracted AppState's Redis pool
            key_plaintext,
            &granularity,
        )
        .await;

        let cached_bundle = match validation_result {
            // Success: resolve_and_validate returned Ok(Ok(bundle))
            Ok(Ok(bundle)) => bundle,

            // Validation Failure: resolve_and_validate returned Ok(Err(validation_error))
            Ok(Err(validation_error)) => return Err(AuthRejection::from(validation_error)),

            // Internal Error: resolve_and_validate returned Err(VaultlessError)
            Err(e) => {
                tracing::error!("Internal resolution error: {:?}", e);
                return Err(AuthRejection(ApiError::from(e)));
            }
        };

        // --- 3. Origin Authorization Check (Strictly enforced for Publishable Keys) ---
        if matches!(granularity, KeyGranularity::Publishable) {
            let required_origin_opt = cached_bundle.application.authorized_origin.as_ref();

            match request_origin {
                Some(origin_value) => {
                    // Origin header is present. Check for DB match if one is required.
                    if let Some(required_origin) = required_origin_opt {
                        if origin_value != required_origin {
                            tracing::warn!(
                                "Origin '{}' not authorized. Expected '{}'",
                                origin_value,
                                required_origin
                            );
                            return Err(AuthRejection::from(ValidationError {
                                type_code: ValidationFailureType::Forbidden,
                                message: "Request origin not authorized.".into(),
                            }));
                        }
                    }
                    // If required_origin_opt is None, we accept the present origin (allowing all).
                }
                None => {
                    // Origin header is missing. REJECT: Publishable keys must supply Origin.
                    let message = "Publishable keys must supply a request 'Origin' header for resource protection. Secret keys can omit this.".to_string();
                    return Err(AuthRejection::from(ValidationError {
                        type_code: ValidationFailureType::Forbidden,
                        message: message.into(),
                    }));
                }
            }
        }

        // --- 4. Success ---
        // Secret keys and validated Publishable keys proceed here.
        Ok(ValidatedKeyBundle(cached_bundle))
    }
}
