use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

/// Client type for cross-referencing application_id
#[derive(Debug, Clone, FromRow)]
pub struct ClientAppRef {
    pub id: Uuid,
    pub application_id: Uuid,
}

/// Active sessions set key
pub fn active_sessions_set() -> String {
    crate::cache_key!("session", "active")
}

/// Constant for active sessions set
pub const ACTIVE_SESSIONS_SET: &str = "session:active";

/// Session key for ephemeral forward-secret communication
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SessionKey {
    pub id: Uuid,
    pub client_id: Uuid,
    pub peer_client_id: Uuid,
    pub application_id: Uuid,
    pub session_id: String,

    /// Ephemeral X25519 public key for this session (base64)
    pub ephemeral_public_key: String,

    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,

    pub messages_sent: i64,
    pub messages_received: i64,
    pub proofs_verified: i64,
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotSession {
    pub session_id: String,
    pub application_id: Uuid,
    pub ephemeral_public_key: String,
    pub expires_at: DateTime<Utc>,

    // HOT counters (never written to SQL directly)
    pub messages_sent: u64,
    pub messages_received: u64,
    pub proofs_verified: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// DTO for creating a new session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionKeyRequest {
    pub client_id: Uuid,
    pub peer_client_id: Uuid,
    pub application_id: Uuid,
    pub session_id: String,
    pub ephemeral_public_key: String,
    pub expires_at: DateTime<Utc>,
}

impl SessionKey {
    /// Create a new session key
    pub async fn create<'c, E>(
        exec: E,
        req: CreateSessionKeyRequest,
    ) -> Result<SessionKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let new_id = Uuid::new_v4();

        let session = sqlx::query_as::<_, SessionKey>(
            "WITH deactivated AS (
                UPDATE session_keys
                SET is_active = false
                WHERE client_id = $1
                  AND peer_client_id = $2
                  AND is_active = true
             )
             INSERT INTO session_keys
             (id, client_id, peer_client_id, application_id, session_id, ephemeral_public_key,
              created_at, expires_at, is_active)
             VALUES ($3, $1, $2, $4, $5, $6, NOW(), $7, true)
             RETURNING *"
        )
        .bind(req.client_id)
        .bind(req.peer_client_id)
        .bind(new_id)
        .bind(req.application_id)
        .bind(req.session_id)
        .bind(req.ephemeral_public_key)
        .bind(req.expires_at)
        .fetch_one(exec)
        .await?;

        Ok(session)
    }

    /// Validate that both clients belong to the same application
    /// Returns the validated application_id if valid, or an error if not
    pub async fn validate_same_application<'c, E>(
        exec: E,
        client_id: Uuid,
        peer_client_id: Uuid,
    ) -> Result<Uuid>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Fetch both clients in a single query
        let clients = sqlx::query_as::<_, ClientAppRef>(
            "SELECT id, application_id FROM public.clients
             WHERE id = $1 OR id = $2"
        )
        .bind(client_id)
        .bind(peer_client_id)
        .fetch_all(exec)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if clients.len() != 2 {
            return Err(VaultlessError::NotFound(
                "One or both clients not found".into(),
            ));
        }

        let client1 = clients.iter().find(|c| c.id == client_id)
            .ok_or_else(|| VaultlessError::NotFound("Client not found".into()))?;
        let client2 = clients.iter().find(|c| c.id == peer_client_id)
            .ok_or_else(|| VaultlessError::NotFound("Peer client not found".into()))?;

        if client1.application_id != client2.application_id {
            return Err(VaultlessError::Forbidden(
                "Clients must belong to the same application to establish a session".into(),
            ));
        }

        Ok(client1.application_id)
    }

    /// Find active session between two clients
    pub async fn find_active<'c, E>(
        exec: E,
        client_id: Uuid,
        peer_client_id: Uuid,
    ) -> Result<Option<SessionKey>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let session = sqlx::query_as::<_, SessionKey>(
            "SELECT * FROM session_keys
             WHERE client_id = $1 AND peer_client_id = $2
               AND is_active = true AND expires_at > NOW()
             ORDER BY created_at DESC
             LIMIT 1"
        )
        .bind(client_id)
        .bind(peer_client_id)
        .fetch_optional(exec)
        .await?;

        Ok(session)
    }

    /// Find session by session_id
    pub async fn find_by_session_id<'c, E>(
        exec: E,
        session_id: &str,
    ) -> Result<Option<SessionKey>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let session = sqlx::query_as::<_, SessionKey>(
            "SELECT * FROM session_keys WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_optional(exec)
        .await?;

        Ok(session)
    }

    /// Update last_used_at timestamp
    pub async fn update_last_used<'c, E>(
        exec: E,
        session_id: &str,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query(
            "UPDATE session_keys SET last_used_at = NOW() WHERE session_id = $1"
        )
        .bind(session_id)
        .execute(exec)
        .await?;

        Ok(())
    }

    /// Deactivate a session
    pub async fn deactivate<'c, E>(
        exec: E,
        session_id: &str,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query(
            "UPDATE session_keys SET is_active = false WHERE session_id = $1"
        )
        .bind(session_id)
        .execute(exec)
        .await?;

        Ok(())
    }

    /// List all active sessions for a client
    pub async fn list_active_sessions<'c, E>(
        exec: E,
        client_id: Uuid,
    ) -> Result<Vec<SessionKey>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let sessions = sqlx::query_as::<_, SessionKey>(
            "SELECT * FROM session_keys
             WHERE client_id = $1 AND is_active = true AND expires_at > NOW()
             ORDER BY created_at DESC"
        )
        .bind(client_id)
        .fetch_all(exec)
        .await?;

        Ok(sessions)
    }

    /// Cleanup expired sessions (called by background job)
    pub async fn cleanup_expired<'c, E>(
        exec: E,
    ) -> Result<i64>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let result = sqlx::query(
            "UPDATE session_keys
             SET is_active = false
             WHERE expires_at < NOW() AND is_active = true"
        )
        .execute(exec)
        .await?;

        Ok(result.rows_affected() as i64)
    }
}

// =============================================================================
// Flusher Module (for Redis to Postgres session counter sync)
// =============================================================================

pub mod flusher;

pub use flusher::{start_session_flusher, SessionFlusherMetrics};
