use super::dto::*;
use crate::crypto;
use crate::error::{Result, VaultlessError};
use crate::models::{ApiKey, CreateApiKey};
use crate::types::KeyType;
use deadpool_redis::Pool as RedisPool;
use sqlx::types::chrono::{DateTime, Utc};
use sqlx::{Acquire, Executor, FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

// --- CRITICAL CHANGE 1: Update PROJECTION to remove deleted columns ---
const PROJECTION: &str = "id, user_id, name, 
    description, is_active, created_at, 
    updated_at, max_ttl_seconds, is_key_rotation_forced, 
    deletion_requested_at, 
    internal_notes, integrity_config";
// Removed: secret_key_id, bundle_id, platform, webhook_url

impl Application {
    /// Create a new application with secret and publishable keys
    pub async fn create<'c, E>(
        exec: E,
        input: CreateApplication,
    ) -> Result<CreateApplicationResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone + Acquire<'c, Database = Postgres>,
    {
        // Start transaction
        let mut tx = exec.begin().await?;

        // Validate input
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // ============================================================
        // 1. ALWAYS GENERATE A NEW SECRET KEY
        // ============================================================
        let secret_key = crypto::generate_api_key("sk", "live")?;
        let secret_key_hash = crypto::hash_content(secret_key.as_bytes());
        let secret_key_prefix = secret_key.chars().take(8).collect::<String>();

        let created_secret_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id: input.user_id,
                key_hash: Some(secret_key_hash),
                key_prefix: secret_key_prefix,
                tier: None,
                description: Some(format!("Secret key for {}", input.name)),
                scopes: None,
                expires_at: None,
                application_id: None, // Assigned later
                key_type: crate::types::KeyType::Secret,
                publishable_key_plaintext: None,
            },
        )
        .await?;

        // ============================================================
        // 2. CREATE THE APPLICATION
        // ============================================================
        let app = sqlx::query_as::<_, Application>(
            r#"
            INSERT INTO applications (
                user_id, 
                name, 
                description, 
                max_ttl_seconds, 
                is_key_rotation_forced, 
                integrity_config
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(input.user_id)
        .bind(&input.name)
        .bind(&input.description)
        .bind(input.max_ttl_seconds.unwrap_or(604800)) // 7 days default
        .bind(input.is_key_rotation_forced.unwrap_or(false))
        .bind(
            input
                .integrity_config
                .unwrap_or_else(|| serde_json::json!({})),
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Application with this name already exists".into())
            }
            _ => VaultlessError::Database(e),
        })?;

        // ============================================================
        // 3. GENERATE PUBLISHABLE KEY
        // ============================================================
        let publishable_key = crypto::generate_api_key("pk", "live")?;
        let pk_prefix = publishable_key.chars().take(16).collect::<String>();

        let created_publishable_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id: input.user_id,
                key_hash: None,
                key_prefix: pk_prefix,
                tier: None,
                description: Some(format!("Publishable key for {}", input.name)),
                scopes: None,
                expires_at: None,
                application_id: None, // assigned later
                key_type: crate::types::KeyType::Publishable,
                publishable_key_plaintext: Some(publishable_key.clone()),
            },
        )
        .await?;

        // ============================================================
        // 4. LINK BOTH API KEYS TO THE NEW APPLICATION
        // ============================================================
        sqlx::query("UPDATE api_keys SET application_id = $1 WHERE id = $2")
            .bind(app.id)
            .bind(created_secret_key.id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE api_keys SET application_id = $1 WHERE id = $2")
            .bind(app.id)
            .bind(created_publishable_key.id)
            .execute(&mut *tx)
            .await?;

        // Commit
        tx.commit().await?;

        tracing::info!(
            application_id = %app.id,
            "Application created with new secret + publishable keys"
        );

        // ============================================================
        // 5. RETURN RESPONSE
        // ============================================================
        Ok(CreateApplicationResponse {
            application: app.into(),
            secret_key: Some(secret_key), // plaintext
            publishable_key_plaintext: publishable_key,
        })
    }

    /// Find application by ID (UNCHANGED logic, but uses new PROJECTION)
    pub async fn find_by_id<'c, E>(exec: E, id: Uuid) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Fetch from DB
        sqlx::query_as::<_, Application>(&format!(
            r#"
                SELECT {}
                FROM applications WHERE id = $1
                "#,
            PROJECTION
        ))
        .bind(id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found.".to_string()))
    }

    pub async fn list_user_applications<'c, E>(
        exec: E,
        user_id: Uuid,
        page: i64,
        page_size: i64,
    ) -> Result<PaginatedApplicationsWithKeys>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let offset = (page - 1).max(0) * page_size;

        // Query the materialized view directly - much faster!
        let rows: Vec<ApplicationWithKeysFromView> =
            sqlx::query_as::<_, ApplicationWithKeysFromView>(
                r#"
        SELECT 
            application_id,
            user_id,
            name,
            description,
            is_active,
            created_at,
            updated_at,
            max_ttl_seconds,
            is_key_rotation_forced,
            deletion_requested_at,
            integrity_config,
            publishable_keys,
            publishable_key_count,
            COUNT(*) OVER() AS total_count
        FROM mv_applications_with_keys
        WHERE user_id = $1
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
            )
            .bind(user_id)
            .bind(page_size)
            .bind(offset)
            .fetch_all(exec)
            .await
            .map_err(VaultlessError::Database)?;

        if rows.is_empty() {
            return Ok(PaginatedApplicationsWithKeys {
                data: vec![],
                total_count: 0,
                page,
                page_size,
                total_pages: 0,
            });
        }

        let total_count = rows[0].total_count;
        let total_pages = (total_count as f64 / page_size as f64).ceil() as i64;

        let data = rows
            .into_iter()
            .map(|r| ApplicationWithKeysResponse {
                id: r.application_id,
                name: r.name,
                description: r.description,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
                max_ttl_seconds: r.max_ttl_seconds,
                is_key_rotation_forced: r.is_key_rotation_forced,
                deletion_requested_at: r.deletion_requested_at,
                internal_notes: r.internal_notes,
                integrity_config: r.integrity_config,
                publishable_keys: r.publishable_keys,
            })
            .collect();

        Ok(PaginatedApplicationsWithKeys {
            data,
            total_count,
            page,
            page_size,
            total_pages,
        })
    }

    /// Find application by publishable key (for client registration) (UNCHANGED logic)
    pub async fn find_by_publishable_key<'c, E>(
        exec: E,
        publishable_key: &str,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Logic remains correct as it JOINs api_keys to find application_id
        // FIXED: Bind key_type as string (assuming enum stored as string)
        let key_type_str = crate::types::KeyType::Publishable.to_string();
        let app = sqlx::query_as::<_, Application>(&format!(
            r#"
            SELECT a.{} FROM applications a
            JOIN api_keys ak ON a.id = ak.application_id
            WHERE ak.publishable_key_plaintext = $1
              AND ak.key_type = $2
              AND a.is_active = true
            "#,
            PROJECTION // Use the updated projection here
        ))
        .bind(publishable_key)
        .bind(&key_type_str)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        Ok(app)
    }

    /// Helper to find the secret key ID associated with this application.
    /// This is needed because `secret_key_id` was removed from the Application struct.
    pub async fn find_secret_key_id<'c, E>(exec: E, app_id: Uuid) -> Result<Uuid>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_scalar(
            r#"
            SELECT id FROM api_keys 
            WHERE application_id = $1 AND key_type = $2
            "#,
        )
        .bind(app_id)
        .bind(KeyType::Secret.to_string())
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Associated Secret Key not found.".to_string()))
    }

    /// Deactivate application (UNCHANGED logic)
    pub async fn deactivate_deep<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
    {
        let row = sqlx::query(
            "UPDATE applications SET is_active = false, updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(exec.clone())
        .await?;

        let Some(_) = row else {
            return Err(VaultlessError::NotFound(format!(
                "Application not found: {id}"
            )));
        };

        let exec_keys = exec.clone();
        sqlx::query(
            "UPDATE api_keys SET is_active = false, updated_at = NOW() WHERE application_id = $1",
        )
        .bind(id)
        .execute(exec_keys)
        .await?;

        if let Some(redis_pool) = redis {
            super::helper::trigger_view_refresh_debounced(exec.clone(), redis_pool.clone());

            tokio::spawn(async move {
                if let Err(e) = Self::invalidate_auth_cache(id, exec, redis_pool).await {
                    tracing::error!("Background cache invalidation failed for app {}: {}", id, e);
                }
            });
        } else {
            tracing::warn!(
                "Redis pool not provided. Skipping cache invalidation for deactivated app {}.",
                id
            );
        }
        Ok(())
    }

    pub async fn deactivate_weak<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
    {
        // 1. Update + return ID if row exists
        let row = sqlx::query!(
            r#"
        UPDATE applications
        SET is_active = false, updated_at = NOW()
        WHERE id = $1
        RETURNING id
        "#,
            id
        )
        .fetch_optional(exec.clone())
        .await?;

        // 2. If not found → return NotFound
        let Some(app_row) = row else {
            return Err(VaultlessError::NotFound(format!(
                "Application not found: {id}"
            )));
        };

        // 3. Invalidate cache using spawned worker
        if let Some(redis_pool) = redis {
            super::helper::trigger_view_refresh_debounced(exec.clone(), redis_pool.clone());
            tokio::spawn(async move {
                if let Err(e) = Self::invalidate_auth_cache(app_row.id, exec, redis_pool).await {
                    tracing::error!(
                        "Background cache invalidation failed for app {}: {}",
                        app_row.id,
                        e
                    );
                }
            });
        }

        Ok(())
    }

    /// Usage is determined by looking up the Secret Key ID linked to the application
    /// and querying the real-time counter associated with that key.
    pub async fn get_monthly_usage<'c, E>(
        exec: E,
        redis_pool: Arc<RedisPool>,
        app_id: Uuid,
    ) -> Result<i64>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        tracing::info!(application_id = %app_id, "Fetching monthly usage for application.");

        // CRITICAL CHANGE 5: Fetch the Secret Key ID dynamically
        let secret_key_id = Self::find_secret_key_id(exec, app_id).await?;

        // Delegate the actual usage lookup to the ApiKey model
        let usage = ApiKey::get_monthly_usage(redis_pool, secret_key_id).await?;

        tracing::info!(
            application_id = %app_id,
            secret_key_id = %secret_key_id,
            usage = usage,
            "Successfully retrieved application monthly usage."
        );

        Ok(usage)
    }
}
