// vaultless-core/src/models/group/types.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

// ============================================================================
// Core Group Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MessageGroup {
    pub id: Uuid,
    pub group_name: Option<String>,
    pub group_type: GroupType,
    pub creator_client_address: Uuid,
    pub allow_member_invite: bool,
    pub require_admin_approval: bool,
    pub max_members: i32,
    
    // E2EE Fields (Sender Keys Protocol)
    pub group_public_key: Option<String>,
    pub encrypted_group_keys: Option<sqlx::types::JsonValue>,
    pub key_version: i32,
    pub uses_sender_keys: bool,
    
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
#[sqlx(type_name = "group_type_enum", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum GroupType {
    Private,
    Public,
    Broadcast,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GroupMember {
    pub id: Uuid,
    pub group_id: Uuid,
    pub client_address: Uuid,
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
    
    // Sender Keys fields
    pub sender_chain_public_key: Option<String>,
    pub sender_key_version: i32,
    
    pub metadata: Option<sqlx::types::JsonValue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "member_role_enum", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum MemberRole {
    Admin,
    Moderator,
    Member,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "member_status_enum", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum MemberStatus {
    Active,
    Muted,
    Left,
    Removed,
    Banned,
}

// ============================================================================
// E2EE Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedGroupKey {
    pub client_id: String,
    pub encrypted_key: String,
    pub key_version: i32,
    pub encrypted_at: String,
}

impl Default for EncryptedGroupKey {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            encrypted_key: String::new(),
            key_version: 0,
            encrypted_at: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderKeyDistribution {
    pub sender_client_id: Uuid,
    pub group_id: Uuid,
    pub chain_key: String,
    pub signing_key: String,
    pub key_id: i32,
    pub encrypted_for_members: Vec<EncryptedSenderKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSenderKey {
    pub recipient_client_id: Uuid,
    pub encrypted_chain_key: String,
}

// ============================================================================
// Pagination Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct PaginatedGroups {
    pub groups: Vec<MessageGroup>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaginatedMembers {
    pub members: Vec<GroupMember>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl PaginationParams {
    pub fn page(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }

    pub fn page_size(&self) -> i64 {
        self.page_size.unwrap_or(20).clamp(1, 100)
    }

    pub fn offset(&self) -> i64 {
        (self.page() - 1) * self.page_size()
    }
}

// ============================================================================
// Request DTOs
// ============================================================================

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateGroup {
    #[validate(length(min = 64, max = 64))]
    pub api_key_hash: String,

    pub creator_client_address: Uuid,

    #[validate(length(max = 255))]
    pub group_name: Option<String>,

    pub group_type: GroupType,

    #[validate(range(min = 2, max = 10000))]
    pub max_members: Option<i32>,

    pub allow_member_invite: Option<bool>,
    pub require_admin_approval: Option<bool>,
    
    pub uses_sender_keys: Option<bool>,
    pub encrypted_group_keys: Vec<EncryptedGroupKey>,
    pub group_public_key: Option<String>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct UpdateGroup {
    #[validate(length(min = 64, max = 64))]
    pub api_key_hash: String,

    #[validate(length(max = 255))]
    pub group_name: Option<String>,

    pub allow_member_invite: Option<bool>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct AddMemberRequest {
    #[validate(length(min = 64, max = 64))]
    pub api_key_hash: String,

    pub client_address: Uuid,
    pub role: Option<MemberRole>,
    pub invited_by: Uuid,
    
    pub encrypted_group_key: Option<EncryptedGroupKey>,
    pub sender_key_distribution: Option<SenderKeyDistribution>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct RotateGroupKeyRequest {
    #[validate(length(min = 64, max = 64))]
    pub api_key_hash: String,

    pub requester_client: Uuid,
    
    #[validate(length(min = 1))]
    pub new_encrypted_keys: Vec<EncryptedGroupKey>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct RemoveMemberRequest {
    #[validate(length(min = 64, max = 64))]
    pub api_key_hash: String,

    pub requester_client: Uuid,
}

// ============================================================================
// Redis Cache Keys
// ============================================================================

pub struct GroupCacheKeys;

impl GroupCacheKeys {
    pub fn group(group_id: &Uuid) -> String {
        format!("group:{}", group_id)
    }

    pub fn group_members(group_id: &Uuid) -> String {
        format!("group:{}:members", group_id)
    }

    pub fn group_member_addresses(group_id: &Uuid) -> String {
        format!("group:{}:member_addresses", group_id)
    }

    pub fn client_groups(client_id: &Uuid) -> String {
        format!("client:{}:groups", client_id)
    }

    pub fn encrypted_key(group_id: &Uuid, client_id: &Uuid) -> String {
        format!("group:{}:key:{}", group_id, client_id)
    }

    pub fn sender_key(group_id: &Uuid, sender_id: &Uuid) -> String {
        format!("group:{}:sender_key:{}", group_id, sender_id)
    }

    pub fn group_member_count(group_id: &Uuid) -> String {
        format!("group:{}:member_count", group_id)
    }
}

// ============================================================================
// Cache TTLs (in seconds)
// ============================================================================

pub struct CacheTTL;

impl CacheTTL {
    pub const GROUP: u64 = 3600;
    pub const GROUP_MEMBERS: u64 = 1800;
    pub const MEMBER_ADDRESSES: u64 = 1800;
    pub const CLIENT_GROUPS: u64 = 1800;
    pub const ENCRYPTED_KEY: u64 = 7200;
    pub const SENDER_KEY: u64 = 3600;
    pub const MEMBER_COUNT: u64 = 600;
}