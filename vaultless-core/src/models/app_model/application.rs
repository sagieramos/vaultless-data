use super::dto::*;
use crate::error::{Result, VaultlessError};
use crate::models::{ApiKey, CreateApiKey};
use crate::types::KeyType;
use crate::{cache_key, crypto};
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::QueryBuilder;
use sqlx::{Acquire, Executor, Postgres, Transaction};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;
const RESOLUTION_CACHE_TTL: u64 = 3600;
const PROJECTION: &str = "id, user_id, name, description, secret_key_id, authorized_origin, bundle_id, platform, webhook_url, created_at, updated_at";

impl Application {
    /// Create a new application with secret and publishable keys
    pub async fn create<'c, E>(
        exec: E,
        input: CreateApplication,
    ) -> Result<CreateApplicationResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone + Acquire<'c, Database = Postgres>,
    {
        // Wrap in transaction for consistency
        let mut tx = exec.begin().await?;

        // 1. Initial Validation
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Variables to hold the final linking ID and the generated secret key (if new)
        let secret_key_id: Uuid; // Renamed for clarity
        let mut generated_secret_key_plaintext: Option<String> = None;

        // --- Determine SECRET Key ID: Existing vs. New ---
        if let Some(existing_id) = input.existing_api_key_id {
            // SCENARIO 1: LINK EXISTING KEY

            // 1. Validate the existing key: Check existence and ownership
            let existing_key = ApiKey::find_by_id(&mut *tx, existing_id).await?;

            if existing_key.user_id != input.user_id {
                return Err(VaultlessError::Forbidden(
                    "API key does not belong to user".into(),
                ));
            }
            // IMPORTANT: Also check key_type if you want to enforce that only 'secret' keys are linked
            if existing_key.key_type != crate::types::KeyType::Secret {
                return Err(VaultlessError::Forbidden(
                    "Only secret keys can be linked as the primary application key.".into(),
                ));
            }
            // NEW: Check if already linked to another application
            if existing_key.application_id.is_some() {
                return Err(VaultlessError::Forbidden(
                    "API key already linked to another application".into(),
                ));
            }

            tracing::info!(
                existing_api_key_id = %existing_id,
                "Linking existing SECRET API key to new application"
            );
            secret_key_id = existing_id;
        } else {
            // SCENARIO 2: CREATE NEW SECRET KEY (Original logic)

            // 1. Generate secret key, hash, and prefix
            let secret_key = crypto::generate_api_key("sk", "live")?;
            let secret_key_hash = crypto::hash_content(secret_key.as_bytes());
            let secret_key_prefix = secret_key.chars().take(8).collect::<String>();

            // 2. Create API key entry
            let secret_api_key = ApiKey::create(
                &mut *tx,
                CreateApiKey {
                    user_id: input.user_id,
                    key_hash: Some(secret_key_hash), // Now Option<String>
                    key_prefix: secret_key_prefix,
                    tier: Some(input.tier), // Now Option<SubscriptionTier>
                    description: Some(["Secret key for ", &input.name].concat()),
                    scopes: None,
                    expires_at: None,
                    application_id: None,
                    // New field: now correctly set to 'secret'
                    key_type: crate::types::KeyType::Secret,
                    // New field: Not used for secret key
                    publishable_key_plaintext: None,
                },
            )
            .await?;

            // Set the ID and store the raw key for the response
            secret_key_id = secret_api_key.id;
            generated_secret_key_plaintext = Some(secret_key);
        };

        // 3. Create application record (MUST BE DONE BEFORE PUBLISHABLE KEY INSERT)
        // SQL change: Renamed secret_key_ref to secret_key_id, removed publishable key fields.
        let app = sqlx::query_as::<_, Application>(
            r#"
            INSERT INTO applications (
                user_id, name, description,
                secret_key_id,
                bundle_id, platform, webhook_url
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(input.user_id)
        .bind(&input.name)
        .bind(&input.description)
        .bind(secret_key_id) // <-- Use the determined SECRET Key ID here
        .bind(&input.bundle_id)
        .bind(&input.platform)
        .bind(&input.webhook_url)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Application with this name already exists".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        // 4. Generate and Create Publishable Key (always new)
        let publishable_key = crypto::generate_api_key("pk", "live")?;
        let publishable_key_prefix = publishable_key.chars().take(16).collect::<String>();

        // We don't need the result, but we need to create the record
        // New ApiKey::create structure used here
        let publishable_api_key = ApiKey::create(
            &mut *tx,
            CreateApiKey {
                user_id: input.user_id,
                key_hash: None, // Null for publishable key
                key_prefix: publishable_key_prefix,
                tier: None, // Null for publishable key
                description: Some({
                    let mut desc = String::with_capacity(24 + input.name.len()); // preallocate buffer
                    desc.push_str("Publishable key for ");
                    desc.push_str(&input.name);
                    desc
                }),
                scopes: None,
                expires_at: None,
                application_id: None,
                key_type: crate::types::KeyType::Publishable,
                // The actual key is stored here
                publishable_key_plaintext: Some(publishable_key.clone()),
            },
        )
        .await?;

        // 5. Update the SECRET key record to reference the newly created application ID
        // Note: The publishable key record created in step 4 should also be updated
        // or the ApiKey::create function should handle linking the application_id
        // if it has access to it. Assuming a separate update for now:
        sqlx::query(r#"UPDATE api_keys SET application_id = $1 WHERE id = $2"#)
            .bind(app.id)
            .bind(secret_key_id)
            .execute(&mut *tx)
            .await?;

        // FIXED: Update the publishable key record's application_id by ID instead of plaintext
        sqlx::query(r#"UPDATE api_keys SET application_id = $1 WHERE id = $2"#)
            .bind(app.id)
            .bind(publishable_api_key.id)
            .execute(&mut *tx)
            .await?;

        // Commit transaction
        tx.commit().await?;

        tracing::info!(
            application_id = %app.id,
            user_id = %input.user_id,
            name = %input.name,
            "Application and associated keys created successfully"
        );

        // 6. Return Response
        Ok(CreateApplicationResponse {
            application: app,
            // Only return the secret key if it was newly generated
            secret_key: generated_secret_key_plaintext,
            // Renamed field in CreateApplicationResponse DTO
            publishable_key_plaintext: publishable_key,
        })
    }

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

    /// Find application by publishable key (for client registration)
    pub async fn find_by_publishable_key<'c, E>(
        exec: E,
        publishable_key: &str,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Database lookup: Now joins api_keys to find the application ID
        // FIXED: Bind key_type as string (assuming enum stored as string)
        let key_type_str = crate::types::KeyType::Publishable.to_string();
        let app = sqlx::query_as::<_, Application>(
            r#"
            SELECT a.* FROM applications a
            JOIN api_keys ak ON a.id = ak.application_id
            WHERE ak.publishable_key_plaintext = $1
              AND ak.key_type = $2
              AND a.is_active = true
            "#,
        )
        .bind(publishable_key)
        .bind(&key_type_str)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        Ok(app)
    }

    /// Resolves the full key bundle using Redis, falling back to DB on cache miss.
    /// Returns the Application, the Secret Key Row, and the type of key the client used.
    /// Resolves an Application Bundle strictly using a Publishable Key (PK) plaintext.
    // In src/models/app_model/application.rs (or wherever these functions reside)
    // NOTE: Assumes CachedResolvedKeyBundle, CachedApplication, and CachedApiKey are defined and in scope.

    /// This path only verifies against the PK cache keys and PK database lookups.
    pub async fn resolve_publishable_key_bundle<'c, E>(
        exec: E,
        redis_pool: Arc<deadpool_redis::Pool>,
        key_plaintext: &str,
    ) -> Result<ResolvedKeyBundle>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let pk_cache_key = publishable_key_resolution_cache_key(key_plaintext);
        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis conn error: {}", e)))?;

        let mut resolved_id: Option<Uuid> = None;

        // --- 1. L1 Resolution Check (PK Cache) ---
        if let Ok(Some(secret_key_id_str)) = conn.get::<_, Option<String>>(&pk_cache_key).await {
            resolved_id = secret_key_id_str.parse::<Uuid>().ok();
            tracing::debug!("Key resolved via PK L1 cache.");
        }

        let secret_key_id = if let Some(id) = resolved_id {
            id
        } else {
            // --- 2. L1 Cache Miss: DB Resolution Fallback ---
            let sk_row = ApiKey::find_by_publishable_key(exec.clone(), key_plaintext)
                .await
                .map_err(|e| {
                    tracing::debug!("DB PK resolution failed: {:?}", e);
                    e
                })?;

            // Prime L1 Cache for this PK (non-blocking, fire-and-forget)
            let cache_id_str = sk_row.id.to_string();
            conn.set_ex::<String, String, ()>(pk_cache_key, cache_id_str, RESOLUTION_CACHE_TTL)
                .await
                .ok();

            sk_row.id
        };

        // --- 3. L2 Bundle Cache Check (MODIFIED for lean cache) ---
        let final_cache_key = cache_key!("app_bundle", secret_key_id);
        if let Ok(Some(bundle_json)) = conn.get::<_, Option<String>>(&final_cache_key).await {
            // Deserialize into the lean cache struct
            let cached_bundle: CachedResolvedKeyBundle = serde_json::from_str(&bundle_json)
                .map_err(|e| {
                    VaultlessError::Internal(format!("Failed to deserialize bundle: {}", e))
                })?;

            tracing::debug!("Lean bundle hit L2 cache (PK).");

            // Reconstruct the full ResolvedKeyBundle (Filling in placeholder values for omitted fields)
            let cached_app = cached_bundle.application;
            let cached_sk = cached_bundle.secret_key_row;

            let app = Self {
                id: cached_app.id,
                user_id: cached_app.user_id,
                // Omitted: name and description
                name: String::new(),
                description: None,
                secret_key_id: cached_app.secret_key_id,
                authorized_origin: cached_app.authorized_origin.clone(), // <-- NEW
                bundle_id: cached_app.bundle_id,
                platform: cached_app.platform,
                webhook_url: cached_app.webhook_url,
                is_active: cached_app.is_active,
                created_at: cached_app.created_at,
                updated_at: cached_app.updated_at,
            };

            let secret_key_row = ApiKey {
                id: cached_sk.id,
                user_id: cached_sk.user_id,
                // Omitted: key_prefix, key_hash, publishable_key_plaintext
                key_prefix: String::new(),
                key_hash: None,
                publishable_key_plaintext: None,
                tier: cached_sk.tier,
                monthly_message_quota: cached_sk.monthly_message_quota,
                message_retention_seconds: cached_sk.message_retention_seconds,
                // Omitted: description, scopes
                description: None,
                scopes: None,
                is_active: cached_sk.is_active,
                created_at: cached_sk.created_at,
                expires_at: cached_sk.expires_at,
                last_used_at: cached_sk.last_used_at,
                rate_limit_per_minute: cached_sk.rate_limit_per_minute,
                application_id: cached_sk.application_id,
                key_type: cached_sk.key_type,
            };

            let bundle = ResolvedKeyBundle {
                application: app,
                secret_key_row,
            };
            return Ok(bundle);
        }

        // --- 4. DB Fetch and Bundle Construction (L2 Cache Miss - UNCHANGED) ---
        let secret_key_row = ApiKey::find_by_id(exec.clone(), secret_key_id).await?;
        let app = Self::find_by_id(
            exec,
            secret_key_row.application_id.ok_or_else(|| {
                VaultlessError::Internal("Secret Key missing app ID.".to_string())
            })?,
        )
        .await?;

        let full_bundle = ResolvedKeyBundle {
            application: app.clone(),
            secret_key_row: secret_key_row.clone(),
        };

        // 5. Cache the lean bundle (L2 Cache Priming - MODIFIED)
        let cached_bundle = CachedResolvedKeyBundle::from(&full_bundle);
        let bundle_json = serde_json::to_string(&cached_bundle)?;
        conn.set_ex::<String, String, ()>(final_cache_key, bundle_json, RESOLUTION_CACHE_TTL)
            .await
            .ok();

        // 6. Return the full bundle
        Ok(full_bundle)
    }

    /// Resolves an Application Bundle strictly using a Secret Key (SK) plaintext.
    /// This path only verifies against the SK hash cache keys and SK database lookups.
    pub async fn resolve_secret_key_bundle<'c, E>(
        exec: E,
        redis_pool: Arc<deadpool_redis::Pool>,
        key_plaintext: &str,
    ) -> Result<ResolvedKeyBundle>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let key_hash = crate::crypto::hash_content(key_plaintext.as_bytes());
        let sk_cache_key = secret_key_resolution_cache_key(&key_hash);
        let key_hash_hex = hex::encode(&key_hash);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis conn error: {}", e)))?;
        let mut resolved_id: Option<Uuid> = None;

        // --- 1. L1 Resolution Check (SK Hash Cache - UNCHANGED) ---
        if let Ok(Some(secret_key_id_str)) = conn.get::<_, Option<String>>(&sk_cache_key).await {
            resolved_id = secret_key_id_str.parse::<Uuid>().ok();
            tracing::debug!("Key resolved via SK L1 cache.");
        }

        let secret_key_id = if let Some(id) = resolved_id {
            id
        } else {
            // --- 2. L1 Cache Miss: DB Resolution Fallback (UNCHANGED) ---
            let sk_row = ApiKey::find_by_hash(exec.clone(), key_hash_hex)
                .await
                .map_err(|e| {
                    tracing::debug!("DB SK resolution failed: {:?}", e);
                    e
                })?;

            // Prime L1 Cache for this SK Hash (non-blocking, fire-and-forget)
            let cache_id_str = sk_row.id.to_string();
            conn.set_ex::<String, String, ()>(sk_cache_key, cache_id_str, RESOLUTION_CACHE_TTL)
                .await
                .ok();

            sk_row.id
        };

        // --- 3. L2 Bundle Cache Check (MODIFIED for lean cache) ---
        let final_cache_key = cache_key!("app_bundle", secret_key_id);
        if let Ok(Some(bundle_json)) = conn.get::<_, Option<String>>(&final_cache_key).await {
            // Deserialize into the lean cache struct
            let cached_bundle: CachedResolvedKeyBundle = serde_json::from_str(&bundle_json)
                .map_err(|e| {
                    VaultlessError::Internal(format!("Failed to deserialize bundle: {}", e))
                })?;

            tracing::debug!("Lean bundle hit L2 cache (SK).");

            // Reconstruct the full ResolvedKeyBundle (Filling in placeholder values for omitted fields)
            let cached_app = cached_bundle.application;
            let cached_sk = cached_bundle.secret_key_row;

            let app = Self {
                id: cached_app.id,
                user_id: cached_app.user_id,
                // Omitted: name and description
                name: String::new(),
                description: None,
                secret_key_id: cached_app.secret_key_id,
                authorized_origin: cached_app.authorized_origin.clone(), // <-- NEW
                bundle_id: cached_app.bundle_id,
                platform: cached_app.platform,
                webhook_url: cached_app.webhook_url,
                is_active: cached_app.is_active,
                created_at: cached_app.created_at,
                updated_at: cached_app.updated_at,
            };

            let secret_key_row = ApiKey {
                id: cached_sk.id,
                user_id: cached_sk.user_id,
                // Omitted: key_prefix, key_hash, publishable_key_plaintext
                key_prefix: String::new(),
                key_hash: None,
                publishable_key_plaintext: None,
                tier: cached_sk.tier,
                monthly_message_quota: cached_sk.monthly_message_quota,
                message_retention_seconds: cached_sk.message_retention_seconds,
                // Omitted: description, scopes
                description: None,
                scopes: None,
                is_active: cached_sk.is_active,
                created_at: cached_sk.created_at,
                expires_at: cached_sk.expires_at,
                last_used_at: cached_sk.last_used_at,
                rate_limit_per_minute: cached_sk.rate_limit_per_minute,
                application_id: cached_sk.application_id,
                key_type: cached_sk.key_type,
            };

            let bundle = ResolvedKeyBundle {
                application: app,
                secret_key_row,
            };
            return Ok(bundle);
        }

        // --- 4. DB Fetch and Bundle Construction (L2 Cache Miss - UNCHANGED) ---
        let secret_key_row = ApiKey::find_by_id(exec.clone(), secret_key_id).await?;
        let app = Self::find_by_id(
            exec,
            secret_key_row.application_id.ok_or_else(|| {
                VaultlessError::Internal("Secret Key missing app ID.".to_string())
            })?,
        )
        .await?;

        // 5. Cache the lean bundle (L2 Cache Priming - MODIFIED)
        let full_bundle = ResolvedKeyBundle {
            application: app.clone(),
            secret_key_row: secret_key_row.clone(),
        };
        let cached_bundle = CachedResolvedKeyBundle::from(&full_bundle);
        let bundle_json = serde_json::to_string(&cached_bundle)?;
        conn.set_ex::<String, String, ()>(final_cache_key, bundle_json, RESOLUTION_CACHE_TTL)
            .await
            .ok();

        // 6. Return the full bundle
        Ok(full_bundle)
    }

    // --- Other methods updated: Find by ID, Deactivate, etc. ---

    /// Deactivate application
    pub async fn deactivate<'c, E>(exec: E, redis: Option<Arc<RedisPool>>, id: Uuid) -> Result<()>
    where
        // E must be Send and 'static to be moved into the spawned task
        E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
    {
        // 1. Find the application to get the necessary details for caching
        // FIXED: Pass exec_find by value (no &mut); assumes find_by_id takes E by value and uses let mut exec = exec;
        let exec_find = exec.clone();
        let app = Self::find_by_id(exec_find, id).await?;

        // 2. Perform critical database updates (MUST be awaited)
        // FIXED: Use &mut exec for each update by creating local mut clones

        // Deactivate application record
        let exec_app = exec.clone();
        sqlx::query("UPDATE applications SET is_active = false, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(exec_app)
            .await?;

        // Deactivate ALL keys associated with this application
        let exec_keys = exec.clone();
        sqlx::query(
            "UPDATE api_keys SET is_active = false, updated_at = NOW() WHERE application_id = $1",
        )
        .bind(id)
        .execute(exec_keys)
        .await?;

        // 3. Cache Invalidation (NON-CRITICAL - use tokio::spawn)

        if let Some(redis_pool) = redis {
            // Clone/move necessary resources into the async task
            let app_clone = app.clone();
            let redis_pool_clone = Arc::clone(&redis_pool);
            let exec_clone = exec.clone(); // Clone the executor for the DB lookup inside invalidation

            tokio::spawn(async move {
                if let Err(e) =
                    Self::invalidate_cache(exec_clone, redis_pool_clone, &app_clone).await
                {
                    tracing::error!(
                        "Background cache invalidation failed for app {}: {}",
                        app_clone.id,
                        e
                    );
                }
            });
        } else {
            tracing::warn!(
                "Redis pool not provided. Skipping cache invalidation for deactivated app {}.",
                id
            );
        }

        // 4. Return immediately after database updates are committed
        Ok(())
    }

    /// Get the associated secret API key (for validation/billing)
    pub async fn get_secret_key<'c, E>(&self, exec: E) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Renamed field: self.secret_key_id
        ApiKey::find_by_id(exec, self.secret_key_id).await
    }

    // ... (validate_secret_key, update_tier, get_tier remain largely the same, using self.secret_key_id)

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

        // 1. Fetch the Application record to get the linked Secret Key ID
        let app = Self::find_by_id(exec, app_id).await?;

        let secret_key_id = app.secret_key_id;

        // 2. Delegate the actual usage lookup to the ApiKey model
        // We use the Secret Key ID as the key for quota tracking.
        let usage = ApiKey::get_monthly_usage(redis_pool, secret_key_id).await?;

        tracing::info!(
            application_id = %app_id,
            secret_key_id = %secret_key_id,
            usage = usage,
            "Successfully retrieved application monthly usage."
        );

        Ok(usage)
    }
    // --- Application Validation Logic ---

    // --- Dynamic Update Logic ---

    /// Update application metadata fields (safe partial update)
    pub async fn update<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        id: Uuid,
        update: UpdateApplication,
    ) -> Result<Application>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. Initialize QueryBuilder
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE applications SET ");
        let mut field_count = 0;

        // 2. Dynamically add fields if they are present in the update struct
        if let Some(name) = &update.name {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("name = ").push_bind(name);
            field_count += 1;
        }
        if let Some(description) = &update.description {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("description = ").push_bind(description);
            field_count += 1;
        }
        if let Some(bundle_id) = &update.bundle_id {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("bundle_id = ").push_bind(bundle_id);
            field_count += 1;
        }
        if let Some(platform) = &update.platform {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("platform = ").push_bind(platform);
            field_count += 1;
        }
        if let Some(webhook_url) = &update.webhook_url {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("webhook_url = ").push_bind(webhook_url);
            field_count += 1;
        }
        if let Some(is_active) = &update.is_active {
            if field_count > 0 {
                qb.push(", ");
            }
            qb.push("is_active = ").push_bind(is_active);
            field_count += 1;
        }

        // Check if any fields were actually updated
        if field_count == 0 {
            tracing::info!(application_id = %id, "Update called with no fields. Skipping database operation.");
            // If no fields were provided, return the current state of the application
            return Self::find_by_id(exec, id).await;
        }

        // 3. Finalize and Execute the Query
        qb.push(" WHERE id = ").push_bind(id).push(" RETURNING *");

        let query = qb.build_query_as::<Application>();
        let updated_app = query.fetch_one(exec.clone()).await?;

        // 4. Cache Invalidation (Non-critical)
        // If Redis is available, attempt cache invalidation.
        // to make it non-critical (i.e., failure won't fail the primary update).
        if let Some(pool) = redis {
            tracing::info!(application_id = %id, "Attempting cache invalidation after application update.");
            let invalidate_result = Self::invalidate_cache(exec.clone(), pool, &updated_app).await;

            if let Err(e) = invalidate_result {
                // Log the error but continue execution
                tracing::debug!(application_id = %id, "Non-critical: Cache invalidation failed: {:?}", e);
            }
        }

        Ok(updated_app)
    }
}
