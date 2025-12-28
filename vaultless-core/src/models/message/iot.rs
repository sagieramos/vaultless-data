//! IoT messaging methods optimized for hot paths.
//!
//! This module provides high-performance messaging for IoT devices with:
//! - Presence tracking (heartbeat-based online detection)
//! - Telemetry (device → server, latest-wins replacement)
//! - Commands (server → device, online-only delivery)
//!
//! All operations are Redis-only for <10ms latency.

use super::dto::{InstantMessage, Message, MessageResponse};
use super::helper::{
    iot_command_key, iot_command_lock_key, iot_presence_key, iot_telemetry_key,
    IOT_COMMAND_TTL_SECS, IOT_PRESENCE_TTL_SECS, IOT_TELEMETRY_TTL_SECS,
};
use crate::error::{Result, VaultlessError};
use chrono::{Duration as ChronoDuration, Utc};
use redis::AsyncCommands;
use tracing::info;
use uuid::Uuid;

impl InstantMessage {
    // =========================================================================
    // Presence Management
    // =========================================================================

    /// Refresh IoT device presence (heartbeat). Call periodically to indicate device is online.
    ///
    /// The device should call this at least every 30 seconds to maintain online status.
    ///
    /// # Returns
    /// * `Ok(true)` - Presence refreshed successfully
    pub async fn iot_heartbeat(&self, device_client_id: Uuid) -> Result<bool> {
        let mut conn = self.redis_pool.get().await?;
        let presence_key = iot_presence_key(device_client_id);

        let _: () = conn
            .set_ex(&presence_key, "1", IOT_PRESENCE_TTL_SECS)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        Ok(true)
    }

    /// Check if an IoT device is currently online.
    ///
    /// # Returns
    /// * `Ok(true)` - Device is online (has valid presence)
    /// * `Ok(false)` - Device is offline (no presence or expired)
    pub async fn iot_is_online(&self, device_client_id: Uuid) -> Result<bool> {
        let mut conn = self.redis_pool.get().await?;
        let presence_key = iot_presence_key(device_client_id);

        let exists: bool = conn
            .exists(&presence_key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        Ok(exists)
    }

    // =========================================================================
    // Telemetry (Device → Server)
    // =========================================================================

    /// Send IoT telemetry from device. Replaces any previous telemetry (latest-wins).
    /// This is optimized for hot paths - Redis only, no DB persistence.
    ///
    /// # Arguments
    /// * `device_client_id` - The IoT device sending telemetry
    /// * `recipient_client_id` - The client that should receive this telemetry
    /// * `ciphertext` - Encrypted telemetry data
    /// * `nonce` - Encryption nonce
    /// * `content_size_bytes` - Size of the content
    /// * `application_id` - API key for billing/tracking
    /// * `envelope_public_key` - Public key for envelope
    ///
    /// # Returns
    /// * `Ok(msg_id)` - Message ID on success
    pub async fn send_iot_telemetry(
        &self,
        device_client_id: Uuid,
        recipient_client_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        content_size_bytes: i64,
        application_id: Uuid,
        envelope_public_key: String,
    ) -> Result<Uuid> {
        let msg_id = Uuid::new_v4();
        let created_at = Utc::now();
        let expires_at = created_at + ChronoDuration::seconds(IOT_TELEMETRY_TTL_SECS as i64);

        let msg = Message {
            id: msg_id,
            ciphertext,
            nonce,
            content_type: Some("application/iot-telemetry".to_string()),
            content_size_bytes,
            application_id,
            created_at,
            expires_at,
            accessed_at: None,
            access_count: 0,
            is_delivered: false,
            delivered_at: None,
            max_access_count: None,
            require_proof_verification: false, // Skip for hot path
            sender_client_id: device_client_id,
            recipient_client_id,
            group_id: None,
            is_group_message: false,
            encryption_algorithm: None,
            algorithm_version: None,
            session_id: None,
            signature: None,
            envelope_public_key,
            file_id: None,
        };

        let mut conn = self.redis_pool.get().await?;

        // Refresh device presence
        let presence_key = iot_presence_key(device_client_id);
        let telemetry_key = iot_telemetry_key(device_client_id);

        // Use pipeline for atomic operation
        let data = serde_json::to_string(&msg)?;

        // Lua script: SET telemetry (replacing previous) + refresh presence atomically
        let lua_script = r#"
            redis.call("SET", KEYS[1], ARGV[1], "EX", ARGV[2])
            redis.call("SET", KEYS[2], "1", "EX", ARGV[3])
            return 1
        "#;

        let _: i32 = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(2)
            .arg(&telemetry_key)
            .arg(&presence_key)
            .arg(&data)
            .arg(IOT_TELEMETRY_TTL_SECS)
            .arg(IOT_PRESENCE_TTL_SECS)
            .query_async(&mut conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        Ok(msg_id)
    }

    /// Fetch latest IoT telemetry for a device.
    ///
    /// # Returns
    /// * `Ok(Some(message))` - Latest telemetry available
    /// * `Ok(None)` - No telemetry available or expired
    pub async fn fetch_iot_telemetry(
        &self,
        device_client_id: Uuid,
    ) -> Result<Option<MessageResponse>> {
        let mut conn = self.redis_pool.get().await?;
        let telemetry_key = iot_telemetry_key(device_client_id);

        let data: Option<String> = conn
            .get(&telemetry_key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        match data {
            Some(json) => {
                let msg: Message = serde_json::from_str(&json)?;
                Ok(Some(MessageResponse::from(msg)))
            }
            None => Ok(None),
        }
    }

    // =========================================================================
    // Commands (Server → Device)
    // =========================================================================

    /// Send command to IoT device. Only succeeds if device is online.
    /// Previous pending command is replaced (latest command wins).
    ///
    /// # Arguments
    /// * `sender_client_id` - The client sending the command
    /// * `device_client_id` - The target IoT device
    /// * `ciphertext` - Encrypted command data
    /// * `nonce` - Encryption nonce
    /// * `content_size_bytes` - Size of the content
    /// * `application_id` - API key for billing/tracking
    /// * `envelope_public_key` - Public key for envelope
    ///
    /// # Returns
    /// * `Ok(msg_id)` - Command queued successfully
    /// * `Err(DeviceOffline)` - Device is not online, command not sent
    pub async fn send_iot_command(
        &self,
        sender_client_id: Uuid,
        device_client_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        content_size_bytes: i64,
        application_id: Uuid,
        envelope_public_key: String,
    ) -> Result<Uuid> {
        let mut conn = self.redis_pool.get().await?;

        // Check if device is online first (fail fast)
        let presence_key = iot_presence_key(device_client_id);
        let is_online: bool = conn
            .exists(&presence_key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if !is_online {
            return Err(VaultlessError::DeviceOffline(device_client_id.to_string()));
        }

        let msg_id = Uuid::new_v4();
        let created_at = Utc::now();
        let expires_at = created_at + ChronoDuration::seconds(IOT_COMMAND_TTL_SECS as i64);

        let msg = Message {
            id: msg_id,
            ciphertext,
            nonce,
            content_type: Some("application/iot-command".to_string()),
            content_size_bytes,
            application_id,
            created_at,
            expires_at,
            accessed_at: None,
            access_count: 0,
            is_delivered: false,
            delivered_at: None,
            max_access_count: Some(1), // Commands are single-delivery
            require_proof_verification: false,
            sender_client_id,
            recipient_client_id: device_client_id,
            group_id: None,
            is_group_message: false,
            encryption_algorithm: None,
            algorithm_version: None,
            session_id: None,
            signature: None,
            envelope_public_key,
            file_id: None,
        };

        let command_key = iot_command_key(device_client_id);
        let data = serde_json::to_string(&msg)?;

        // Atomic: check presence + set command (only if still online)
        let lua_script = r#"
            local presence_key = KEYS[1]
            local command_key = KEYS[2]

            -- Double-check device is still online
            if redis.call("EXISTS", presence_key) == 0 then
                return 0
            end

            -- Set command with TTL (replaces any previous command)
            redis.call("SET", command_key, ARGV[1], "EX", ARGV[2])
            return 1
        "#;

        let result: i32 = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(2)
            .arg(&presence_key)
            .arg(&command_key)
            .arg(&data)
            .arg(IOT_COMMAND_TTL_SECS)
            .query_async(&mut conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if result == 0 {
            return Err(VaultlessError::DeviceOffline(device_client_id.to_string()));
        }

        info!(
            device = %device_client_id,
            msg_id = %msg_id,
            "IoT command queued"
        );

        Ok(msg_id)
    }

    /// Fetch pending command for IoT device. Atomically retrieves and deletes.
    /// Also refreshes device presence (acts as heartbeat).
    ///
    /// # Returns
    /// * `Ok(Some(message))` - Command was pending and delivered
    /// * `Ok(None)` - No command pending
    pub async fn fetch_iot_command(
        &self,
        device_client_id: Uuid,
    ) -> Result<Option<MessageResponse>> {
        let mut conn = self.redis_pool.get().await?;

        let presence_key = iot_presence_key(device_client_id);
        let command_key = iot_command_key(device_client_id);
        let lock_key = iot_command_lock_key(device_client_id);

        // Atomic: refresh presence + get-and-delete command + prevent duplicates
        let lua_script = r#"
            local presence_key = KEYS[1]
            local command_key = KEYS[2]
            local lock_key = KEYS[3]
            local presence_ttl = tonumber(ARGV[1])

            -- Refresh presence (heartbeat)
            redis.call("SET", presence_key, "1", "EX", presence_ttl)

            -- Try to acquire lock (prevents duplicate fetch in race conditions)
            if redis.call("SET", lock_key, "1", "NX", "EX", 2) == nil then
                return nil
            end

            -- Get and delete command atomically
            local cmd = redis.call("GET", command_key)
            if cmd then
                redis.call("DEL", command_key)
            end

            -- Release lock
            redis.call("DEL", lock_key)

            return cmd
        "#;

        let result: Option<String> = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(3)
            .arg(&presence_key)
            .arg(&command_key)
            .arg(&lock_key)
            .arg(IOT_PRESENCE_TTL_SECS)
            .query_async(&mut conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        match result {
            Some(json) => {
                let msg: Message = serde_json::from_str(&json)?;
                info!(
                    device = %device_client_id,
                    msg_id = %msg.id,
                    "IoT command delivered"
                );
                Ok(Some(MessageResponse::from(msg)))
            }
            None => Ok(None),
        }
    }
}
