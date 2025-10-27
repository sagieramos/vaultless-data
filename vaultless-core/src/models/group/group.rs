// vaultless-core/src/models/group/group.rs

use redis::AsyncCommands;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};
use crate::models::api_key::ApiKey;

use super::types::*;

impl MessageGroup {
    /// Create new group with E2EE (uses transactions + caching)
    pub async fn create(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        input: CreateGroup,
    ) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Validate API key
        let api_key = ApiKey::find_by_hash(pool, &input.api_key_hash).await?;
        api_key.validate()?;

        // Start transaction
        let mut tx = pool.begin().await?;

        // Verify creator client exists
        let client_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM clients WHERE id = $1 AND is_active = true)",
        )
        .bind(input.creator_client_address)
        .fetch_one(&mut *tx)
        .await?;

        if !client_exists {
            return Err(VaultlessError::NotFound("Client not found".to_string()));
        }

        // Validate encrypted keys include creator
        if !input
            .encrypted_group_keys
            .iter()
            .any(|k| k.client_id == input.creator_client_address.to_string())
        {
            return Err(VaultlessError::Validation(
                "Encrypted group key for creator is required".to_string(),
            ));
        }

        // Determine if we should use sender keys (default: groups > 10 members)
        let uses_sender_keys = input
            .uses_sender_keys
            .unwrap_or(input.encrypted_group_keys.len() > 10);

        // Convert encrypted keys to JSON
        let encrypted_keys_json = json!({
            "keys": input.encrypted_group_keys
        });

        // Create group
        let group = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO message_groups (
                creator_client_address, group_name, group_type,
                max_members, allow_member_invite, require_admin_approval,
                group_public_key, encrypted_group_keys, key_version, uses_sender_keys
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, $9)
            RETURNING *
            "#,
        )
        .bind(input.creator_client_address)
        .bind(&input.group_name)
        .bind(input.group_type)
        .bind(input.max_members.unwrap_or(100))
        .bind(input.allow_member_invite.unwrap_or(false))
        .bind(input.require_admin_approval.unwrap_or(true))
        .bind(&input.group_public_key)
        .bind(encrypted_keys_json)
        .bind(uses_sender_keys)
        .fetch_one(&mut *tx)
        .await?;

        // Add creator as admin member (within transaction)
        Self::add_member_internal_tx(
            &mut tx,
            group.id,
            input.creator_client_address,
            MemberRole::Admin,
            None,
        )
        .await?;

        // Commit transaction
        tx.commit().await?;

        // Cache the new group
        let group_json = serde_json::to_string(&group)
            .map_err(|e| VaultlessError::Internal(format!("Serialization error: {}", e)))?;
        
        let _: () = redis
            .set_ex(
                GroupCacheKeys::group(&group.id),
                group_json,
                CacheTTL::GROUP,
            )
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

        // Cache creator's groups list (add to set)
        let _: () = redis
            .sadd(
                GroupCacheKeys::client_groups(&input.creator_client_address),
                group.id.to_string(),
            )
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

        let _: () = redis
            .expire(
                GroupCacheKeys::client_groups(&input.creator_client_address),
                CacheTTL::CLIENT_GROUPS as i64,
            )
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

        Ok(group)
    }

    /// Find group by ID with caching
    pub async fn find_by_id(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        id: Uuid,
    ) -> Result<Self> {
        let cache_key = GroupCacheKeys::group(&id);

        // Try cache first
        let cached: Option<String> = redis
            .get(&cache_key)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

        if let Some(json_str) = cached {
            let group: Self = serde_json::from_str(&json_str)
                .map_err(|e| VaultlessError::Internal(format!("Deserialization error: {}", e)))?;
            return Ok(group);
        }

        // Cache miss - fetch from DB
        let group = sqlx::query_as::<_, Self>(
            "SELECT * FROM message_groups WHERE id = $1 AND is_active = true",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Group not found".to_string()))?;

        // Cache for next time
        let group_json = serde_json::to_string(&group)
            .map_err(|e| VaultlessError::Internal(format!("Serialization error: {}", e)))?;

        let _: () = redis
            .set_ex(cache_key, group_json, CacheTTL::GROUP)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

        Ok(group)
    }

    /// List groups for a client with pagination and caching
    pub async fn list_for_client_paginated(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        client_address: Uuid,
        params: PaginationParams,
    ) -> Result<PaginatedGroups> {
        let page = params.page();
        let page_size = params.page_size();
        let offset = params.offset();

        // Try to get from cache (only first page)
        if page == 1 {
            let cache_key = GroupCacheKeys::client_groups(&client_address);
            let cached_ids: Vec<String> = redis
                .smembers(&cache_key)
                .await
                .unwrap_or_default();

            if !cached_ids.is_empty() && cached_ids.len() <= page_size as usize {
                // Fetch groups from cache
                let mut groups = Vec::new();
                for id_str in cached_ids.iter().take(page_size as usize) {
                    if let Ok(group_id) = Uuid::parse_str(id_str) {
                        if let Ok(group) = Self::find_by_id(pool, redis, group_id).await {
                            groups.push(group);
                        }
                    }
                }

                if !groups.is_empty() {
                    return Ok(PaginatedGroups {
                        total: groups.len() as i64,
                        page,
                        page_size,
                        has_more: false,
                        groups,
                    });
                }
            }
        }

        // Cache miss or not first page - fetch from DB
        let total: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM message_groups g
            INNER JOIN group_members gm ON g.id = gm.group_id
            WHERE gm.client_address = $1
                AND gm.status = 'active'
                AND g.is_active = true
            "#,
        )
        .bind(client_address)
        .fetch_one(pool)
        .await?;

        let groups = sqlx::query_as::<_, Self>(
            r#"
            SELECT g.* FROM message_groups g
            INNER JOIN group_members gm ON g.id = gm.group_id
            WHERE gm.client_address = $1
                AND gm.status = 'active'
                AND g.is_active = true
            ORDER BY g.last_message_at DESC NULLS LAST
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(client_address)
        .bind(page_size)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        // Cache first page results
        if page == 1 && !groups.is_empty() {
            let cache_key = GroupCacheKeys::client_groups(&client_address);
            let group_ids: Vec<String> = groups.iter().map(|g| g.id.to_string()).collect();

            let _: () = redis
                .del(&cache_key)
                .await
                .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

            if !group_ids.is_empty() {
                let _: () = redis
                    .sadd(&cache_key, group_ids)
                    .await
                    .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

                let _: () = redis
                    .expire(&cache_key, CacheTTL::CLIENT_GROUPS as i64)
                    .await
                    .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;
            }
        }

        let has_more = (offset + page_size) < total;

        Ok(PaginatedGroups {
            groups,
            total,
            page,
            page_size,
            has_more,
        })
    }

    /// Get encrypted group key for a specific client (lazy-loaded with caching)
    pub async fn get_encrypted_key_for_client(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<EncryptedGroupKey> {
        let cache_key = GroupCacheKeys::encrypted_key(&group_id, &client_address);

        // Try cache first
        let cached: Option<String> = redis
            .get(&cache_key)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

        if let Some(json_str) = cached {
            let key: EncryptedGroupKey = serde_json::from_str(&json_str)
                .map_err(|e| VaultlessError::Internal(format!("Deserialization error: {}", e)))?;
            return Ok(key);
        }

        // Cache miss - fetch from DB
        let group = Self::find_by_id(pool, redis, group_id).await?;

        let encrypted_keys = group
            .encrypted_group_keys
            .ok_or_else(|| VaultlessError::NotFound("No encrypted keys found".to_string()))?;

        let keys_map: serde_json::Value = serde_json::from_value(encrypted_keys)
            .map_err(|e| VaultlessError::Internal(format!("Failed to parse encrypted keys: {}", e)))?;

        let keys_array = keys_map["keys"]
            .as_array()
            .ok_or_else(|| VaultlessError::Internal("Invalid encrypted keys format".to_string()))?;

        for key_value in keys_array {
            let key: EncryptedGroupKey = serde_json::from_value(key_value.clone())
                .map_err(|e| VaultlessError::Internal(format!("Failed to parse key: {}", e)))?;

            if key.client_id == client_address.to_string() {
                // Cache the key
                let key_json = serde_json::to_string(&key).map_err(|e| {
                    VaultlessError::Internal(format!("Serialization error: {}", e))
                })?;

                let _: () = redis
                    .set_ex(cache_key, key_json, CacheTTL::ENCRYPTED_KEY)
                    .await
                    .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

                return Ok(key);
            }
        }

        Err(VaultlessError::NotFound(
            "Encrypted key not found for client".to_string(),
        ))
    }

    /// Get member addresses with caching
    pub async fn get_member_addresses(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
    ) -> Result<Vec<Uuid>> {
        let cache_key = GroupCacheKeys::group_member_addresses(&group_id);

        // Try cache first
        let cached: Vec<String> = redis
            .smembers(&cache_key)
            .await
            .unwrap_or_default();

        if !cached.is_empty() {
            let addresses: Vec<Uuid> = cached
                .iter()
                .filter_map(|s| Uuid::parse_str(s).ok())
                .collect();

            if !addresses.is_empty() {
                return Ok(addresses);
            }
        }

        // Cache miss - fetch from DB
        let addresses: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT client_address FROM group_members
            WHERE group_id = $1 AND status = 'active'
            "#,
        )
        .bind(group_id)
        .fetch_all(pool)
        .await?;

        // Cache the addresses
        if !addresses.is_empty() {
            let address_strs: Vec<String> = addresses.iter().map(|a| a.to_string()).collect();

            let _: () = redis
                .del(&cache_key)
                .await
                .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

            let _: () = redis
                .sadd(&cache_key, address_strs)
                .await
                .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;

            let _: () = redis
                .expire(&cache_key, CacheTTL::MEMBER_ADDRESSES as i64)
                .await
                .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;
        }

        Ok(addresses)
    }

    /// Check if client is member (with caching)
    pub async fn is_member(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<bool> {
        // Check cache first
        let cache_key = GroupCacheKeys::group_member_addresses(&group_id);
        let is_cached_member: bool = redis
            .sismember(&cache_key, client_address.to_string())
            .await
            .unwrap_or(false);

        if is_cached_member {
            return Ok(true);
        }

        // Check DB
        let is_member: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM group_members
                WHERE group_id = $1 AND client_address = $2 AND status = 'active'
            )
            "#,
        )
        .bind(group_id)
        .bind(client_address)
        .fetch_one(pool)
        .await?;

        Ok(is_member)
    }

    /// Check if client can send messages (with caching)
    pub async fn can_send_message(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<bool> {
        let cache_key = format!("group:{}:can_send:{}", group_id, client_address);

        // Try cache
        let cached: Option<bool> = redis.get(&cache_key).await.ok();

        if let Some(can_send) = cached {
            return Ok(can_send);
        }

        // DB check
        let can_send: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT can_send_messages FROM group_members
            WHERE group_id = $1 AND client_address = $2 AND status = 'active'
            "#,
        )
        .bind(group_id)
        .bind(client_address)
        .fetch_optional(pool)
        .await?;

        let result = can_send.unwrap_or(false);

        // Cache result
        let _: () = redis
            .set_ex(&cache_key, result, 600)
            .await
            .unwrap_or(());

        Ok(result)
    }

    /// Rotate group key with transaction
    pub async fn rotate_group_key(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        input: RotateGroupKeyRequest,
    ) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Validate API key
        let api_key = ApiKey::find_by_hash(pool, &input.api_key_hash).await?;
        api_key.validate()?;

        // Start transaction
        let mut tx = pool.begin().await?;

        // Check if requester is admin
        let is_admin: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM group_members
                WHERE group_id = $1 
                    AND client_address = $2 
                    AND status = 'active'
                    AND role = 'admin'
            )
            "#,
        )
        .bind(group_id)
        .bind(input.requester_client)
        .fetch_one(&mut *tx)
        .await?;

        if !is_admin {
            return Err(VaultlessError::Forbidden(
                "Only admins can rotate group keys".to_string(),
            ));
        }

        // Get all active members
        let active_members: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT client_address FROM group_members
            WHERE group_id = $1 AND status = 'active'
            "#,
        )
        .bind(group_id)
        .fetch_all(&mut *tx)
        .await?;

        // Verify that new encrypted keys cover all active members
        let provided_client_ids: Vec<Uuid> = input
            .new_encrypted_keys
            .iter()
            .filter_map(|k| Uuid::parse_str(&k.client_id).ok())
            .collect();

        for member_id in &active_members {
            if !provided_client_ids.contains(member_id) {
                return Err(VaultlessError::Validation(format!(
                    "Missing encrypted key for client {}",
                    member_id
                )));
            }
        }

        // Increment key version and update encrypted keys
        let encrypted_keys_json = json!({
            "keys": input.new_encrypted_keys
        });

        let group = sqlx::query_as::<_, Self>(
            r#"
            UPDATE message_groups
            SET 
                encrypted_group_keys = $2,
                key_version = key_version + 1,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(group_id)
        .bind(encrypted_keys_json)
        .fetch_one(&mut *tx)
        .await?;

        // Commit transaction
        tx.commit().await?;

        // Invalidate all related caches
        Self::invalidate_group_caches(redis, group_id, &active_members).await?;

        Ok(group)
    }

    /// Update group (with transaction and cache invalidation)
    pub async fn update(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        requester_client: Uuid,
        input: UpdateGroup,
    ) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Validate API key
        let api_key = ApiKey::find_by_hash(pool, &input.api_key_hash).await?;
        api_key.validate()?;

        let mut tx = pool.begin().await?;

        // Check if requester is admin
        let is_admin: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM group_members
                WHERE group_id = $1 
                    AND client_address = $2 
                    AND status = 'active'
                    AND role = 'admin'
            )
            "#,
        )
        .bind(group_id)
        .bind(requester_client)
        .fetch_one(&mut *tx)
        .await?;

        if !is_admin {
            return Err(VaultlessError::Forbidden(
                "Only admins can update group".to_string(),
            ));
        }

        let group = sqlx::query_as::<_, Self>(
            r#"
            UPDATE message_groups
            SET 
                group_name = COALESCE($2, group_name),
                allow_member_invite = COALESCE($3, allow_member_invite),
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(group_id)
        .bind(input.group_name)
        .bind(input.allow_member_invite)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        // Invalidate group cache
        let _: () = redis
            .del(GroupCacheKeys::group(&group_id))
            .await
            .unwrap_or(());

        Ok(group)
    }

    /// Archive group (with transaction)
    pub async fn archive(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        requester_client: Uuid,
        api_key_hash: &str,
    ) -> Result<()> {
        // Validate API key
        let api_key = ApiKey::find_by_hash(pool, api_key_hash).await?;
        api_key.validate()?;

        let mut tx = pool.begin().await?;

        // Check if requester is admin
        let is_admin: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM group_members
                WHERE group_id = $1 
                    AND client_address = $2 
                    AND status = 'active'
                    AND role = 'admin'
            )
            "#,
        )
        .bind(group_id)
        .bind(requester_client)
        .fetch_one(&mut *tx)
        .await?;

        if !is_admin {
            return Err(VaultlessError::Forbidden(
                "Only admins can archive group".to_string(),
            ));
        }

        sqlx::query(
            r#"
            UPDATE message_groups
            SET is_archived = true, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(group_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Invalidate all group caches
        let members = Self::get_member_addresses(pool, redis, group_id).await?;
        Self::invalidate_group_caches(redis, group_id, &members).await?;

        Ok(())
    }

    // ========================================================================
    // Internal Helper Methods
    // ========================================================================

    /// Add member internal (within transaction)
    pub(super) async fn add_member_internal_tx(
        tx: &mut Transaction<'_, Postgres>,
        group_id: Uuid,
        client_address: Uuid,
        role: MemberRole,
        invited_by: Option<Uuid>,
    ) -> Result<GroupMember> {
        let (can_send, can_add, can_remove) = match role {
            MemberRole::Admin => (true, true, true),
            MemberRole::Moderator => (true, true, false),
            MemberRole::Member => (true, false, false),
        };

        let member = sqlx::query_as::<_, GroupMember>(
            r#"
            INSERT INTO group_members (
                group_id, client_address, role,
                can_send_messages, can_add_members, can_remove_members,
                invited_by_client_address
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (group_id, client_address) 
            DO UPDATE SET 
                status = 'active',
                role = EXCLUDED.role,
                can_send_messages = EXCLUDED.can_send_messages,
                can_add_members = EXCLUDED.can_add_members,
                can_remove_members = EXCLUDED.can_remove_members,
                joined_at = NOW(),
                left_at = NULL
            RETURNING *
            "#,
        )
        .bind(group_id)
        .bind(client_address)
        .bind(role)
        .bind(can_send)
        .bind(can_add)
        .bind(can_remove)
        .bind(invited_by)
        .fetch_one(&mut **tx)
        .await?;

        Ok(member)
    }

    /// Invalidate all caches related to a group
    pub async fn invalidate_group_caches(
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        member_ids: &[Uuid],
    ) -> Result<()> {
        // Collect all cache keys to delete
        let mut keys_to_delete = vec![
            GroupCacheKeys::group(&group_id),
            GroupCacheKeys::group_members(&group_id),
            GroupCacheKeys::group_member_addresses(&group_id),
            GroupCacheKeys::group_member_count(&group_id),
        ];

        // Add client-specific caches
        for client_id in member_ids {
            keys_to_delete.push(GroupCacheKeys::client_groups(client_id));
            keys_to_delete.push(GroupCacheKeys::encrypted_key(&group_id, client_id));
            keys_to_delete.push(GroupCacheKeys::sender_key(&group_id, client_id));
            keys_to_delete.push(format!("group:{}:can_send:{}", group_id, client_id));
        }

        // Delete all keys
        if !keys_to_delete.is_empty() {
            let _: () = redis
                .del(keys_to_delete)
                .await
                .map_err(|e| VaultlessError::Internal(format!("Redis error: {}", e)))?;
        }

        Ok(())
    }
}