// vaultless-core/src/models/group.rs
use super::message::Message;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MessageGroup {
    pub id: Uuid,
    pub group_name: Option<String>,
    pub group_type: GroupType,
    pub creator_client_address: Uuid,
    pub creator_user_id: Uuid,
    pub allow_member_invite: bool,
    pub require_admin_approval: bool,
    pub max_members: Option<i32>,
    pub group_public_key: Option<String>,
    pub is_active: bool,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub member_count: i32,
    pub message_count: i32,
    pub metadata: Option<sqlx::types::JsonValue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "text")]
#[serde(rename_all = "lowercase")]
pub enum GroupType {
    Private,   // Invite-only
    Public,    // Anyone can join
    Broadcast, // Only admins can post
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GroupMember {
    pub id: Uuid,
    pub group_id: Uuid,
    pub client_address: Uuid,
    pub user_id: Uuid,
    pub role: MemberRole,
    pub can_send_messages: bool,
    pub can_add_members: bool,
    pub can_remove_members: bool,
    pub status: MemberStatus,
    pub joined_at: DateTime<Utc>,
    pub left_at: Option<DateTime<Utc>>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub unread_count: i32,
    pub invited_by_client_address: Option<Uuid>,
    pub metadata: Option<sqlx::types::JsonValue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "text")]
#[serde(rename_all = "lowercase")]
pub enum MemberRole {
    Admin,
    Moderator,
    Member,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "text")]
#[serde(rename_all = "lowercase")]
pub enum MemberStatus {
    Active,
    Muted,
    Left,
    Removed,
    Banned,
}

impl MessageGroup {
    /// Create new group
    pub async fn create(
        pool: &PgPool,
        creator_address: Uuid,
        creator_user_id: Uuid,
        group_name: Option<String>,
        group_type: GroupType,
    ) -> Result<Self> {
        let group_id: Uuid = sqlx::query_scalar("SELECT create_message_group($1, $2, $3, $4)")
            .bind(creator_address)
            .bind(creator_user_id)
            .bind(group_name)
            .bind(group_type)
            .fetch_one(pool)
            .await?;

        Self::find_by_id(pool, group_id).await
    }

    /// Find group by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let group = sqlx::query_as::<_, Self>("SELECT * FROM message_groups WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Group not found".to_string()))?;

        Ok(group)
    }

    /// List groups for a client
    pub async fn list_for_client(pool: &PgPool, client_address: Uuid) -> Result<Vec<Self>> {
        let groups = sqlx::query_as::<_, Self>(
            r#"
            SELECT g.* FROM message_groups g
            INNER JOIN group_members gm ON g.id = gm.group_id
            WHERE gm.client_address = $1
                AND gm.status = 'active'
                AND g.is_active = TRUE
            ORDER BY g.last_message_at DESC NULLS LAST
            "#,
        )
        .bind(client_address)
        .fetch_all(pool)
        .await?;

        Ok(groups)
    }

    /// Add member to group
    pub async fn add_member(
        pool: &PgPool,
        group_id: Uuid,
        client_address: Uuid,
        user_id: Uuid,
        invited_by: Uuid,
    ) -> Result<GroupMember> {
        let member_id: Uuid = sqlx::query_scalar("SELECT add_group_member($1, $2, $3, $4)")
            .bind(group_id)
            .bind(client_address)
            .bind(user_id)
            .bind(invited_by)
            .fetch_one(pool)
            .await?;

        GroupMember::find_by_id(pool, member_id).await
    }

    /// Get group members
    pub async fn get_members(pool: &PgPool, group_id: Uuid) -> Result<Vec<GroupMember>> {
        let members = sqlx::query_as::<_, GroupMember>(
            r#"
            SELECT * FROM group_members
            WHERE group_id = $1 AND status = 'active'
            ORDER BY joined_at ASC
            "#,
        )
        .bind(group_id)
        .fetch_all(pool)
        .await?;

        Ok(members)
    }

    /// Get member addresses (for sending messages)
    pub async fn get_member_addresses(pool: &PgPool, group_id: Uuid) -> Result<Vec<Uuid>> {
        let addresses: Vec<Uuid> = sqlx::query_scalar("SELECT get_group_member_addresses($1)")
            .bind(group_id)
            .fetch_one(pool)
            .await?;

        Ok(addresses)
    }

    /// Check if client is member
    pub async fn is_member(pool: &PgPool, group_id: Uuid, client_address: Uuid) -> Result<bool> {
        let is_member: bool = sqlx::query_scalar("SELECT is_group_member($1, $2)")
            .bind(group_id)
            .bind(client_address)
            .fetch_one(pool)
            .await?;

        Ok(is_member)
    }
}

impl GroupMember {
    /// Find member by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let member = sqlx::query_as::<_, Self>("SELECT * FROM group_members WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Member not found".to_string()))?;

        Ok(member)
    }

    /// Leave group
    pub async fn leave_group(pool: &PgPool, group_id: Uuid, client_address: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE group_members
            SET status = 'left', left_at = NOW()
            WHERE group_id = $1 AND client_address = $2
            "#,
        )
        .bind(group_id)
        .bind(client_address)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark message as read
    pub async fn mark_message_read(
        pool: &PgPool,
        message_id: Uuid,
        group_id: Uuid,
        client_address: Uuid,
    ) -> Result<()> {
        sqlx::query("SELECT mark_group_message_read($1, $2, $3)")
            .bind(message_id)
            .bind(group_id)
            .bind(client_address)
            .execute(pool)
            .await?;

        Ok(())
    }
}

// Update Message model
impl Message {
    /// Send group message
    pub async fn send_to_group(
        pool: &PgPool,
        sender_address: Uuid,
        group_id: Uuid,
        ciphertext: String,
        nonce: String,
        api_key_id: Uuid,
        content_size_bytes: i32,
        expires_at: DateTime<Utc>,
    ) -> Result<Self> {
        // Verify sender is group member
        if !MessageGroup::is_member(pool, group_id, sender_address).await? {
            return Err(VaultlessError::Unauthorized(
                "Not a group member".to_string(),
            ));
        }

        // Create message
        let message = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO messages (
                sender_client_address, group_id, is_group_message,
                ciphertext, nonce, api_key_id, content_size_bytes, expires_at
            )
            VALUES ($1, $2, TRUE, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(sender_address)
        .bind(group_id)
        .bind(ciphertext)
        .bind(nonce)
        .bind(api_key_id)
        .bind(content_size_bytes)
        .bind(expires_at)
        .fetch_one(pool)
        .await?;

        Ok(message)
    }

    /// Get group messages
    pub async fn get_group_messages(
        pool: &PgPool,
        group_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>> {
        let messages = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM messages
            WHERE group_id = $1 AND expires_at > NOW()
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(group_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(messages)
    }
}
