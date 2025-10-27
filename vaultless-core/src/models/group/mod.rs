// vaultless-core/src/models/group/mod.rs
//! Group messaging with E2EE support using Sender Keys Protocol
//! 
//! This module implements secure group messaging with:
//! - Sender Keys Protocol for forward secrecy
//! - Redis caching for performance
//! - Message reactions (encrypted)
//! - File sharing with separate keys
//! - Real-time E2EE support

pub mod types;
pub mod group;
pub mod member;
pub mod sender_keys;
pub mod reactions;
pub mod files;

pub use types::{
    AddMemberRequest, CacheTTL, CreateGroup, EncryptedGroupKey, EncryptedSenderKey,
    GroupCacheKeys, GroupMember, GroupType, MemberRole, MemberStatus, MessageGroup,
    PaginatedGroups, PaginatedMembers, PaginationParams, RemoveMemberRequest,
    RotateGroupKeyRequest, SenderKeyDistribution, UpdateGroup,
};
pub use group::*;
pub use member::*;
pub use sender_keys::*;
pub use reactions::*;
pub use files::*;