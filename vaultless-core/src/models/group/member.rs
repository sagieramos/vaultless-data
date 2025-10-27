// vaultless-core/src/models/group/member.rs

use redis::AsyncCommands;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Result, VaultlessError};
use crate::models::api_key::ApiKey;

use super::types::*;

impl GroupMember {
    /// Add member to group with E2EE (with transaction and cache invalidation)
    pub async fn add_member(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        input: AddMemberRequest,
    ) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Validate API key
        let api_key = ApiKey::find_by_hash(pool, &input.api_key_hash).await?;
        api_key.validate()?;

        let mut tx = pool.begin().await?;

        // Check if inviter has permission
        let inviter = Self::find_by_group_and_client_tx(&mut tx, group_id, input.invited_by).await?;

        if inviter.status != MemberStatus::Active {
            return Err(VaultlessError::Forbidden(
                "Inviter is not an active member".to_string(),
            ));
        }

        if !inviter.can_add_members {
            return Err(VaultlessError::Forbidden(
                "You don't have permission to add members".to_string(),
            ));
        }

        // Check if group has space
        let group: MessageGroup = sqlx::query_as(
            "SELECT * FROM message_groups WHERE id = $1 FOR UPDATE"
        )
        .bind(group_id)
        .fetch_one(&mut *tx)
        .await?;

        if group.member_count >= group.max_members {
            return Err(VaultlessError::Validation("Group is full".to_string()));
        }

        // Check if client exists
        let client_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM clients WHERE id = $1 AND is_active = true)",
        )
        .bind(input.client_address)
        .fetch_one(&mut *tx)
        .await?;

        if !client_exists {
            return Err(VaultlessError::NotFound("Client not found".to_string()));
        }

        // Handle E2EE key distribution
        if group.uses_sender_keys {
            // Sender Keys Protocol: distribute sender keys
            if let Some(sender_key_dist) = input.sender_key_distribution {
                Self::store_sender_key_distribution(&mut tx, &sender_key_dist).await?;
            } else {
                return Err(VaultlessError::Validation(
                    "sender_key_distribution required for this group".to_string(),
                ));
            }
        } else {
            // Shared Key: add encrypted group key
            if let Some(encrypted_key) = input.encrypted_group_key {
                // Verify key is for correct client
                if encrypted_key.client_id != input.client_address.to_string() {
                    return Err(VaultlessError::Validation(
                        "Encrypted key client_id mismatch".to_string(),
                    ));
                }

                // Add to group's encrypted_group_keys
                let mut encrypted_keys = group
                    .encrypted_group_keys
                    .clone()
                    .unwrap_or_else(|| json!({"keys": []}));

                if let Some(keys_array) = encrypted_keys["keys"].as_array_mut() {
                    keys_array.push(json!(encrypted_key));
                }

                sqlx::query(
                    r#"
                    UPDATE message_groups
                    SET encrypted_group_keys = $2, updated_at = NOW()
                    WHERE id = $1
                    "#,
                )
                .bind(group_id)
                .bind(encrypted_keys)
                .execute(&mut *tx)
                .await?;
            } else {
                return Err(VaultlessError::Validation(
                    "encrypted_group_key required for this group".to_string(),
                ));
            }
        }

        let role = input.role.unwrap_or(MemberRole::Member);

        let member = MessageGroup::add_member_internal_tx(
            &mut tx,
            group_id,
            input.client_address,
            role,
            Some(input.invited_by),
        )
        .await?;

        tx.commit().await?;

        // Invalidate caches
        let members = MessageGroup::get_member_addresses(pool, redis, group_id).await?;
        MessageGroup::invalidate_group_caches(redis, group_id, &members).await?;

        Ok(member)
    }

    /// Get paginated members with caching
    pub async fn get_members_paginated(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        params: PaginationParams,
    ) -> Result<PaginatedMembers> {
        let page = params.page();
        let page_size = params.page_size();
        let offset = params.offset();

        // Try cache for first page only
        if page == 1 {
            let cache_key = GroupCacheKeys::group_members(&group_id);
            let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();

            if let Some(json_str) = cached {
                if let Ok(members) = serde_json::from_str::<Vec<GroupMember>>(&json_str) {
                    let total = members.len() as i64;
                    let members = members.into_iter().take(page_size as usize).collect();
                    
                    return Ok(PaginatedMembers {
                        members,
                        total,
                        page,
                        page_size,
                        has_more: total > page_size,
                    });
                }
            }
        }

        // Fetch from DB
        let total: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM group_members
            WHERE group_id = $1 AND status = 'active'
            "#,
        )
        .bind(group_id)
        .fetch_one(pool)
        .await?;

        let members = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM group_members
            WHERE group_id = $1 AND status = 'active'
            ORDER BY role DESC, joined_at ASC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(group_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        // Cache first page
        if page == 1 && !members.is_empty() {
            // Fetch all members for caching
            let all_members: Vec<GroupMember> = sqlx::query_as(
                r#"
                SELECT * FROM group_members
                WHERE group_id = $1 AND status = 'active'
                ORDER BY role DESC, joined_at ASC
                "#,
            )
            .bind(group_id)
            .fetch_all(pool)
            .await?;

            let cache_key = GroupCacheKeys::group_members(&group_id);
            if let Ok(json_str) = serde_json::to_string(&all_members) {
                let _: () = redis
                    .set_ex(cache_key, json_str, CacheTTL::GROUP_MEMBERS)
                    .await
                    .unwrap_or(());
            }
        }

        let has_more = (offset + page_size) < total;

        Ok(PaginatedMembers {
            members,
            total,
            page,
            page_size,
            has_more,
        })
    }

    /// Find member by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let member = sqlx::query_as::<_, Self>("SELECT * FROM group_members WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Member not found".to_string()))?;

        Ok(member)
    }

    /// Get member by group and client
    pub async fn find_by_group_and_client(
        pool: &PgPool,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<Self> {
        let member = sqlx::query_as::<_, Self>(
            "SELECT * FROM group_members WHERE group_id = $1 AND client_address = $2",
        )
        .bind(group_id)
        .bind(client_address)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Member not found".to_string()))?;

        Ok(member)
    }

    /// Get member by group and client (within transaction)
    async fn find_by_group_and_client_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<Self> {
        let member = sqlx::query_as::<_, Self>(
            "SELECT * FROM group_members WHERE group_id = $1 AND client_address = $2",
        )
        .bind(group_id)
        .bind(client_address)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Member not found".to_string()))?;

        Ok(member)
    }

    /// Leave group (with transaction)
    pub async fn leave_group(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;

        sqlx::query(
            r#"
            UPDATE group_members
            SET status = 'left', left_at = NOW()
            WHERE group_id = $1 AND client_address = $2
            "#,
        )
        .bind(group_id)
        .bind(client_address)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Invalidate caches
        let members = MessageGroup::get_member_addresses(pool, redis, group_id).await?;
        MessageGroup::invalidate_group_caches(redis, group_id, &members).await?;

        Ok(())
    }

    /// Remove member from group (with transaction)
    pub async fn remove_member(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        target_client: Uuid,
        requester_client: Uuid,
        api_key_hash: &str,
    ) -> Result<()> {
        // Validate API key
        let api_key = ApiKey::find_by_hash(pool, api_key_hash).await?;
        api_key.validate()?;

        let mut tx = pool.begin().await?;

        // Check if requester has permission
        let requester = Self::find_by_group_and_client_tx(&mut tx, group_id, requester_client).await?;

        if !requester.can_remove_members {
            return Err(VaultlessError::Forbidden(
                "You don't have permission to remove members".to_string(),
            ));
        }

        sqlx::query(
            r#"
            UPDATE group_members
            SET status = 'removed', left_at = NOW()
            WHERE group_id = $1 AND client_address = $2
            "#,
        )
        .bind(group_id)
        .bind(target_client)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Invalidate caches
        let members = MessageGroup::get_member_addresses(pool, redis, group_id).await?;
        MessageGroup::invalidate_group_caches(redis, group_id, &members).await?;

        Ok(())
    }

    /// Update member role (with transaction)
    pub async fn update_role(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        target_client: Uuid,
        new_role: MemberRole,
        requester_client: Uuid,
        api_key_hash: &str,
    ) -> Result<Self> {
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
                "Only admins can update roles".to_string(),
            ));
        }

        let (can_send, can_add, can_remove) = match new_role {
            MemberRole::Admin => (true, true, true),
            MemberRole::Moderator => (true, true, false),
            MemberRole::Member => (true, false, false),
        };

        let member = sqlx::query_as::<_, Self>(
            r#"
            UPDATE group_members
            SET 
                role = $3,
                can_send_messages = $4,
                can_add_members = $5,
                can_remove_members = $6
            WHERE group_id = $1 AND client_address = $2
            RETURNING *
            "#,
        )
        .bind(group_id)
        .bind(target_client)
        .bind(new_role)
        .bind(can_send)
        .bind(can_add)
        .bind(can_remove)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        // Invalidate caches
        let _: () = redis
            .del(GroupCacheKeys::group_members(&group_id))
            .await
            .unwrap_or(());

        Ok(member)
    }

    /// Mark messages as read (with caching)
    pub async fn mark_messages_read(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE group_members
            SET last_read_at = NOW(), unread_count = 0
            WHERE group_id = $1 AND client_address = $2
            "#,
        )
        .bind(group_id)
        .bind(client_address)
        .execute(pool)
        .await?;

        // Invalidate member cache
        let _: () = redis
            .del(GroupCacheKeys::group_members(&group_id))
            .await
            .unwrap_or(());

        Ok(())
    }

    // ========================================================================
    // Sender Keys Protocol Support
    // ========================================================================

    /// Store sender key distribution (within transaction)
    async fn store_sender_key_distribution(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        distribution: &SenderKeyDistribution,
    ) -> Result<()> {
        // Store sender's public signing key
        sqlx::query(
            r#"
            UPDATE group_members
            SET 
                sender_chain_public_key = $3,
                sender_key_version = $4
            WHERE group_id = $1 AND client_address = $2
            "#,
        )
        .bind(distribution.group_id)
        .bind(distribution.sender_client_id)
        .bind(&distribution.signing_key)
        .bind(distribution.key_id)
        .execute(&mut **tx)
        .await?;

        // Store encrypted chain keys for each recipient
        for encrypted_key in &distribution.encrypted_for_members {
            sqlx::query(
                r#"
                INSERT INTO sender_keys (
                    group_id, sender_client_id, recipient_client_id, 
                    encrypted_chain_key, key_version
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (group_id, sender_client_id, recipient_client_id)
                DO UPDATE SET 
                    encrypted_chain_key = EXCLUDED.encrypted_chain_key,
                    key_version = EXCLUDED.key_version,
                    updated_at = NOW()
                "#,
            )
            .bind(distribution.group_id)
            .bind(distribution.sender_client_id)
            .bind(encrypted_key.recipient_client_id)
            .bind(&encrypted_key.encrypted_chain_key)
            .bind(distribution.key_id)
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }
}