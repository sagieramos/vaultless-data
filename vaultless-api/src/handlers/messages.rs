use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::state::AppState;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vaultless_core::VaultlessError;

use vaultless_core::{CreateMessage, Message, PaginatedMessages};

// App State: Shared pool and any other shared resources (e.g., API key validator)
#[derive(Clone)]
// Query params for pagination
#[derive(Deserialize, Default)]
pub struct PaginationParams {
    pub limit: Option<i64>,
    pub after: Option<DateTime<Utc>>, // ISO string, e.g., "2023-01-01T00:00:00Z"
}

// API Response wrappers for consistency (with status, optional errors)
#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub data: T,
    pub message: Option<String>,
}
type ApiResult<T> = std::result::Result<Json<ApiResponse<T>>, (StatusCode, Json<ApiResponse<()>>)>;

// POST /messages - Create a message
pub async fn create_message(
    State(state): State<AppState>,
    Json(input): Json<CreateMessage>,
) -> ApiResult<Message> {
    // Best practice: Validate input early (already in model, but add auth if needed, e.g., extract API key from header)
    // Assume API key validated via middleware/extractor

    let message = Message::create(&state.db, input).await.map_err(|e| {
        let status = match e {
            VaultlessError::Validation(_) => StatusCode::BAD_REQUEST,
            VaultlessError::QuotaExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
            VaultlessError::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ApiResponse {
                data: (),
                message: Some(format!("Failed to create message: {}", e)),
            }),
        )
    })?;

    Ok(Json(ApiResponse {
        data: message,
        message: Some("Message created successfully".to_string()),
    }))
}

// GET /messages/recipient/:client_id - Paginated messages for recipient client
pub async fn get_messages_by_recipient_client(
    State(state): State<AppState>,
    Path(client_id): Path<Uuid>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<PaginatedMessages> {
    let paginated = Message::find_paginated_by_recipient_client(
        &state.db,
        client_id,
        params.limit.unwrap_or(20),
        params.after,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                data: (),
                message: Some(format!("Failed to fetch messages: {}", e)),
            }),
        )
    })?;

    Ok(Json(ApiResponse {
        data: paginated,
        message: None,
    }))
}

// GET /messages/sender/:client_id - Paginated sent messages
pub async fn get_messages_by_sender_client(
    State(state): State<AppState>,
    Path(client_id): Path<Uuid>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<PaginatedMessages> {
    let paginated = Message::find_paginated_by_sender_client(
        &state.db,
        client_id,
        params.limit.unwrap_or(20),
        params.after,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                data: (),
                message: Some(format!("Failed to fetch sent messages: {}", e)),
            }),
        )
    })?;

    Ok(Json(ApiResponse {
        data: paginated,
        message: None,
    }))
}

// GET /messages/conversation/:client1_id/:client2_id - Paginated conversation
pub async fn get_conversation_messages(
    State(state): State<AppState>,
    Path((client1_id, client2_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<PaginatedMessages> {
    let paginated = Message::find_paginated_by_conversation(
        &state.db,
        client1_id,
        client2_id,
        params.limit.unwrap_or(20),
        params.after,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                data: (),
                message: Some(format!("Failed to fetch conversation: {}", e)),
            }),
        )
    })?;

    Ok(Json(ApiResponse {
        data: paginated,
        message: None,
    }))
}

// GET /messages/:id - Get single message
pub async fn get_message_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Message> {
    let message = Message::find_by_id(&state.db, id).await.map_err(|e| {
        let status = match e {
            VaultlessError::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ApiResponse {
                data: (),
                message: Some(format!("Message not found or error: {}", e)),
            }),
        )
    })?;

    // Best practice: Check access rights here (e.g., if caller is recipient_client_id)
    // message.validate_access()?;  // If needed, handle error

    Ok(Json(ApiResponse {
        data: message,
        message: None,
    }))
}

// PUT /messages/:id/access - Mark accessed (with optional proof)
#[derive(Deserialize)]
pub struct AccessRequest {
    pub proof: Option<String>, // e.g., JWT or sig
}

pub async fn mark_message_accessed(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<AccessRequest>,
) -> ApiResult<Message> {
    let message = Message::mark_accessed(&state.db, id, req.proof.as_deref())
        .await
        .map_err(|e| {
            let status = match e {
                VaultlessError::Validation(_) | VaultlessError::InvalidProof => {
                    StatusCode::BAD_REQUEST
                }
                VaultlessError::MessageExpired | VaultlessError::MessageAccessLimitReached => {
                    StatusCode::FORBIDDEN
                }
                VaultlessError::NotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status,
                Json(ApiResponse {
                    data: (),
                    message: Some(format!("Access failed: {}", e)),
                }),
            )
        })?;

    Ok(Json(ApiResponse {
        data: message,
        message: Some("Message accessed successfully".to_string()),
    }))
}

// PUT /messages/:id/delivered - Mark delivered
pub async fn mark_message_delivered(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Message> {
    let message = Message::mark_delivered(&state.db, id).await.map_err(|e| {
        let status = match e {
            VaultlessError::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ApiResponse {
                data: (),
                message: Some(format!("Delivery update failed: {}", e)),
            }),
        )
    })?;

    Ok(Json(ApiResponse {
        data: message,
        message: Some("Message marked as delivered".to_string()),
    }))
}

// DELETE /messages/expired - Admin cleanup (optional auth)
pub async fn cleanup_expired_messages(State(state): State<AppState>) -> ApiResult<u64> {
    let count = Message::cleanup_expired(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                data: (),
                message: Some(format!("Cleanup failed: {}", e)),
            }),
        )
    })?;

    Ok(Json(ApiResponse {
        data: count,
        message: Some(format!("Cleaned up {} expired messages", count)),
    }))
}

// Router setup (in main.rs or mod)
use axum::{
    Router,
    routing::{delete, get, post, put},
};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/messages", post(create_message))
        .route(
            "/messages/recipient/:client_id",
            get(get_messages_by_recipient_client),
        )
        .route(
            "/messages/sender/:client_id",
            get(get_messages_by_sender_client),
        )
        .route(
            "/messages/conversation/:client1_id/:client2_id",
            get(get_conversation_messages),
        )
        .route("/messages/:id", get(get_message_by_id))
        .route("/messages/:id/access", put(mark_message_accessed))
        .route("/messages/:id/delivered", put(mark_message_delivered))
        .route("/messages/expired", delete(cleanup_expired_messages))
        .with_state(state)
}

// Best practices applied:
// - Async handlers with proper error mapping to HTTP statuses.
// - Extractors: State for shared pool, Path/Json/Query for params.
// - Pagination via query params (limit/after cursor).
// - JSON responses wrapped for consistency.
// - Validation/auth deferred to middleware (e.g., tower::Service for API keys).
// - No direct DB in handlers—use model methods.
// - Graceful errors: Specific statuses (400, 403, 404, 429, 500).
// - Security: Proof in access; assume middleware checks caller vs recipient_client_id.
// - Docs: Add OpenAPI/Swagger if needed via utoipa.
