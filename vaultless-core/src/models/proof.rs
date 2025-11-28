use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
use validator::Validate;

use crate::error::{Result, VaultlessError};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MessageProof {
    pub id: Uuid,
    pub message_id: Uuid,
    pub content_hash: String,
    pub signature: String,
    pub public_key: String,
    pub algorithm: String,
    pub hash_algorithm: String,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub verification_count: i32,
    pub proof_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateProof {
    pub message_id: Uuid,

    #[validate(length(equal = 64))] // SHA-256 hex = 64 chars
    pub content_hash: String,

    #[validate(length(min = 1))]
    pub signature: String,

    #[validate(length(min = 1))]
    pub public_key: String,

    pub algorithm: Option<String>,
    pub hash_algorithm: Option<String>,
    pub proof_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyProofRequest {
    pub message_id: Uuid,
    pub content_hash: String,
    pub signature: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProofVerificationResult {
    pub is_valid: bool,
    pub message_id: Uuid,
    pub verified_at: DateTime<Utc>,
    pub proof_id: Uuid,
}

impl MessageProof {
    /// Create a new proof
    pub async fn create(pool: &PgPool, input: CreateProof) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Verify message exists
        let proof = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO message_proofs (
                message_id, content_hash, signature, public_key, 
                algorithm, hash_algorithm, proof_metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(input.message_id)
        .bind(&input.content_hash)
        .bind(&input.signature)
        .bind(&input.public_key)
        .bind(input.algorithm.as_deref().unwrap_or("Ed25519"))
        .bind(input.hash_algorithm.as_deref().unwrap_or("SHA-256"))
        .bind(input.proof_metadata)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_foreign_key_violation() => {
                VaultlessError::NotFound("Message not found".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        Ok(proof)
    }

    /// Find proof by message ID
    pub async fn find_by_message_id(pool: &PgPool, message_id: Uuid) -> Result<Self> {
        let proof = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM message_proofs WHERE message_id = $1
            "#,
        )
        .bind(message_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Proof not found".to_string()))?;

        Ok(proof)
    }

    /// Find proof by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let proof = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM message_proofs WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Proof not found".to_string()))?;

        Ok(proof)
    }

    /// Find proof by content hash
    pub async fn find_by_content_hash(pool: &PgPool, content_hash: &str) -> Result<Vec<Self>> {
        let proofs = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM message_proofs 
            WHERE content_hash = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(content_hash)
        .fetch_all(pool)
        .await?;

        Ok(proofs)
    }

    /// Mark proof as verified
    pub async fn mark_verified(pool: &PgPool, id: Uuid) -> Result<Self> {
        let proof = sqlx::query_as::<_, Self>(
            r#"
            UPDATE message_proofs 
            SET 
                verification_count = verification_count + 1,
                verified_at = COALESCE(verified_at, NOW())
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(proof)
    }

    /// Verify proof cryptographically (this will call crypto module later)
    pub async fn verify(
        pool: &PgPool,
        request: VerifyProofRequest,
    ) -> Result<ProofVerificationResult> {
        // Find the proof
        let proof = Self::find_by_message_id(pool, request.message_id).await?;

        // Validate proof data matches
        if proof.content_hash != request.content_hash {
            return Err(VaultlessError::InvalidProof);
        }

        if proof.signature != request.signature {
            return Err(VaultlessError::InvalidProof);
        }

        if proof.public_key != request.public_key {
            return Err(VaultlessError::InvalidProof);
        }

        // TODO: Add cryptographic verification here using ed25519-dalek
        // For now, we just validate the data matches

        // Mark as verified
        let updated_proof = Self::mark_verified(pool, proof.id).await?;

        Ok(ProofVerificationResult {
            is_valid: true,
            message_id: request.message_id,
            verified_at: updated_proof.verified_at.unwrap_or_else(Utc::now),
            proof_id: updated_proof.id,
        })
    }

    /// Get verification statistics for a message
    pub async fn get_verification_stats(
        pool: &PgPool,
        message_id: Uuid,
    ) -> Result<VerificationStats> {
        let stats = sqlx::query_as::<_, VerificationStats>(
            r#"
            SELECT 
                message_id,
                verification_count,
                verified_at IS NOT NULL as has_been_verified,
                created_at as first_proof_created,
                verified_at as first_verified_at
            FROM message_proofs 
            WHERE message_id = $1
            "#,
        )
        .bind(message_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("No proof found for message".to_string()))?;

        Ok(stats)
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct VerificationStats {
    pub message_id: Uuid,
    pub verification_count: i32,
    pub has_been_verified: bool,
    pub first_proof_created: DateTime<Utc>,
    pub first_verified_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_proof_validation() {
        let invalid_hash = CreateProof {
            message_id: Uuid::new_v4(),
            content_hash: "short".to_string(), // Should be 64 chars
            signature: "sig".to_string(),
            public_key: "key".to_string(),
            algorithm: None,
            hash_algorithm: None,
            proof_metadata: None,
        };

        assert!(invalid_hash.validate().is_err());
    }
}
