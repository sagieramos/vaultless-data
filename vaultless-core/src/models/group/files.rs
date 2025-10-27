// vaultless-core/src/models/group/files.rs

use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
use validator::Validate;

use crate::error::{Result, VaultlessError};

// ============================================================================
// File Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GroupFile {
    pub id: Uuid,
    pub group_id: Uuid,
    pub message_id: Option<Uuid>,
    pub uploader_client_id: Uuid,
    
    // File metadata (encrypted)
    pub encrypted_filename: String,
    pub encrypted_mime_type: String,
    pub file_size_bytes: i64,
    
    // Encryption info
    pub encrypted_file_key: String,
    pub nonce: String,
    
    // Storage
    pub storage_path: String,
    pub chunk_count: i32,
    
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub download_count: i32,
    pub max_downloads: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileChunk {
    pub id: Uuid,
    pub file_id: Uuid,
    pub chunk_index: i32,
    pub encrypted_data: Vec<u8>,
    pub chunk_size_bytes: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct UploadFileRequest {
    pub group_id: Uuid,
    pub uploader_client_id: Uuid,
    pub message_id: Option<Uuid>,
    
    #[validate(length(min = 1))]
    pub encrypted_filename: String,
    
    #[validate(length(min = 1))]
    pub encrypted_mime_type: String,
    
    #[validate(range(min = 1, max = 104857600))]
    pub file_size_bytes: i64,
    
    #[validate(length(min = 1))]
    pub encrypted_file_key: String,
    
    #[validate(length(min = 1, max = 32))]
    pub nonce: String,
    
    #[validate(length(min = 1))]
    pub storage_path: String,
    
    pub chunk_count: Option<i32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_downloads: Option<i32>,
}

impl GroupFile {
    /// Upload file metadata (with transaction)
    pub async fn create(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        input: UploadFileRequest,
    ) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        let mut tx = pool.begin().await?;

        // Verify uploader is group member
        let is_member: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM group_members
                WHERE group_id = $1 
                    AND client_address = $2 
                    AND status = 'active'
                    AND can_send_messages = true
            )
            "#,
        )
        .bind(input.group_id)
        .bind(input.uploader_client_id)
        .fetch_one(&mut *tx)
        .await?;

        if !is_member {
            return Err(VaultlessError::Forbidden(
                "Not authorized to upload files to this group".to_string(),
            ));
        }

        // Create file record
        let file = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO group_files (
                group_id, message_id, uploader_client_id,
                encrypted_filename, encrypted_mime_type, file_size_bytes,
                encrypted_file_key, nonce, storage_path, chunk_count,
                expires_at, max_downloads
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING *
            "#,
        )
        .bind(input.group_id)
        .bind(input.message_id)
        .bind(input.uploader_client_id)
        .bind(&input.encrypted_filename)
        .bind(&input.encrypted_mime_type)
        .bind(input.file_size_bytes)
        .bind(&input.encrypted_file_key)
        .bind(&input.nonce)
        .bind(&input.storage_path)
        .bind(input.chunk_count.unwrap_or(1))
        .bind(input.expires_at)
        .bind(input.max_downloads)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        // Cache file metadata
        let cache_key = format!("file:{}", file.id);
        if let Ok(json_str) = serde_json::to_string(&file) {
            let _: () = redis
                .set_ex(&cache_key, json_str, 3600)
                .await
                .unwrap_or(());
        }

        Ok(file)
    }

    /// Get file by ID (with caching)
    pub async fn find_by_id(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        file_id: Uuid,
    ) -> Result<Self> {
        let cache_key = format!("file:{}", file_id);

        // Try cache first
        let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();

        if let Some(json_str) = cached {
            if let Ok(file) = serde_json::from_str::<Self>(&json_str) {
                return Ok(file);
            }
        }

        // Fetch from DB
        let file = sqlx::query_as::<_, Self>(
            "SELECT * FROM group_files WHERE id = $1"
        )
        .bind(file_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("File not found".to_string()))?;

        // Cache it
        if let Ok(json_str) = serde_json::to_string(&file) {
            let _: () = redis
                .set_ex(&cache_key, json_str, 3600)
                .await
                .unwrap_or(());
        }

        Ok(file)
    }

    /// Get files for a group (paginated)
    pub async fn list_for_group(
        pool: &PgPool,
        group_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>> {
        let files = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM group_files
            WHERE group_id = $1
                AND (expires_at IS NULL OR expires_at > NOW())
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(group_id)
        .bind(limit.clamp(1, 100))
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(files)
    }

    /// Get files uploaded by a client
    pub async fn list_by_uploader(
        pool: &PgPool,
        uploader_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>> {
        let files = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM group_files
            WHERE uploader_client_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(uploader_id)
        .bind(limit.clamp(1, 100))
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(files)
    }

    /// Increment download count and check limits
    pub async fn record_download(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        file_id: Uuid,
    ) -> Result<Self> {
        let mut tx = pool.begin().await?;

        let file = sqlx::query_as::<_, Self>(
            r#"
            UPDATE group_files
            SET download_count = download_count + 1
            WHERE id = $1
                AND (max_downloads IS NULL OR download_count < max_downloads)
                AND (expires_at IS NULL OR expires_at > NOW())
            RETURNING *
            "#,
        )
        .bind(file_id)
        .fetch_optional(&mut *tx)
        .await?;

        let file = file.ok_or_else(|| {
            VaultlessError::Forbidden("File download limit reached or expired".to_string())
        })?;

        tx.commit().await?;

        // Invalidate cache
        let cache_key = format!("file:{}", file_id);
        let _: () = redis.del(&cache_key).await.unwrap_or(());

        Ok(file)
    }

    /// Delete file (soft delete by setting expired)
    pub async fn delete(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        file_id: Uuid,
        requester_id: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;

        // Check if requester is uploader or group admin
        let can_delete: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM group_files gf
                LEFT JOIN group_members gm ON gf.group_id = gm.group_id 
                    AND gm.client_address = $2
                WHERE gf.id = $1
                    AND (gf.uploader_client_id = $2 OR gm.role = 'admin')
            )
            "#,
        )
        .bind(file_id)
        .bind(requester_id)
        .fetch_one(&mut *tx)
        .await?;

        if !can_delete {
            return Err(VaultlessError::Forbidden(
                "Not authorized to delete this file".to_string(),
            ));
        }

        // Soft delete by setting expires_at to now
        sqlx::query(
            r#"
            UPDATE group_files
            SET expires_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(file_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Invalidate cache
        let cache_key = format!("file:{}", file_id);
        let _: () = redis.del(&cache_key).await.unwrap_or(());

        Ok(())
    }

    /// Cleanup expired files
    pub async fn cleanup_expired(pool: &PgPool) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM group_files WHERE expires_at < NOW()"
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}

impl FileChunk {
    /// Store file chunk (for large files)
    pub async fn store_chunk(
        pool: &PgPool,
        file_id: Uuid,
        chunk_index: i32,
        encrypted_data: Vec<u8>,
    ) -> Result<Self> {
        let chunk_size = encrypted_data.len() as i32;

        let chunk = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO file_chunks (
                file_id, chunk_index, encrypted_data, chunk_size_bytes
            )
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (file_id, chunk_index)
            DO UPDATE SET 
                encrypted_data = EXCLUDED.encrypted_data,
                chunk_size_bytes = EXCLUDED.chunk_size_bytes
            RETURNING *
            "#,
        )
        .bind(file_id)
        .bind(chunk_index)
        .bind(&encrypted_data)
        .bind(chunk_size)
        .fetch_one(pool)
        .await?;

        Ok(chunk)
    }

    /// Get file chunk
    pub async fn get_chunk(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        file_id: Uuid,
        chunk_index: i32,
    ) -> Result<Self> {
        let cache_key = format!("file:{}:chunk:{}", file_id, chunk_index);

        // Try cache (for small chunks only)
        let cached: Option<Vec<u8>> = redis.get(&cache_key).await.ok();

        if let Some(_data) = cached {
            // For simplicity, we fetch from DB anyway
            // In production, cache the entire chunk struct
        }

        // Fetch from DB
        let chunk = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM file_chunks
            WHERE file_id = $1 AND chunk_index = $2
            "#,
        )
        .bind(file_id)
        .bind(chunk_index)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Chunk not found".to_string()))?;

        // Cache small chunks (< 1MB)
        if chunk.chunk_size_bytes < 1048576 {
            let _: () = redis
                .set_ex(&cache_key, &chunk.encrypted_data, 600)
                .await
                .unwrap_or(());
        }

        Ok(chunk)
    }

    /// Get all chunks for a file (for download)
    pub async fn get_all_for_file(
        pool: &PgPool,
        file_id: Uuid,
    ) -> Result<Vec<Self>> {
        let chunks = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM file_chunks
            WHERE file_id = $1
            ORDER BY chunk_index ASC
            "#,
        )
        .bind(file_id)
        .fetch_all(pool)
        .await?;

        Ok(chunks)
    }

    /// Delete chunks for a file
    pub async fn delete_for_file(pool: &PgPool, file_id: Uuid) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM file_chunks WHERE file_id = $1"
        )
        .bind(file_id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}