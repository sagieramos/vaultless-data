# End-to-End Encryption for Group Messaging - Implementation Guide

## Overview

This guide explains how to implement end-to-end encrypted (E2EE) group messaging in your Vaultless system using a **shared group key** approach.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    E2EE Group Messaging Flow                     │
└─────────────────────────────────────────────────────────────────┘

1. GROUP CREATION
   ┌──────────┐
   │ Client A │ Generates group_symmetric_key (AES-256)
   │(Creator) │ Encrypts group_key with each member's public_key
   └────┬─────┘
        │
        ├─→ encrypt(group_key, client_A_public_key) → encrypted_key_A
        ├─→ encrypt(group_key, client_B_public_key) → encrypted_key_B
        └─→ encrypt(group_key, client_C_public_key) → encrypted_key_C
        
   Store in DB: {
     "keys": [
       {"client_id": "A", "encrypted_key": "...", "key_version": 1},
       {"client_id": "B", "encrypted_key": "...", "key_version": 1},
       {"client_id": "C", "encrypted_key": "...", "key_version": 1}
     ]
   }

2. SENDING MESSAGE
   ┌──────────┐
   │ Client A │ 
   └────┬─────┘
        │ 1. Retrieve own encrypted_group_key
        │ 2. Decrypt with private_key → group_key
        │ 3. Encrypt message: ciphertext = AES(message, group_key, nonce)
        │ 4. Store: { ciphertext, nonce, group_id, sender_id }
        └─→ Database

3. RECEIVING MESSAGE
   ┌──────────┐
   │ Client B │
   └────┬─────┘
        │ 1. Fetch encrypted_group_key for self
        │ 2. Decrypt with private_key → group_key
        │ 3. Fetch message: { ciphertext, nonce }
        │ 4. Decrypt: message = AES_decrypt(ciphertext, group_key, nonce)
        └─→ Plaintext message

4. KEY ROTATION (when member leaves)
   ┌──────────┐
   │ Admin    │ 
   └────┬─────┘
        │ 1. Generate NEW group_symmetric_key
        │ 2. Encrypt for remaining members only
        │ 3. Increment key_version to 2
        │ 4. Update encrypted_group_keys in DB
        └─→ Old messages still readable with old key_version
```

## Client-Side Implementation

### 1. Group Creation (Client-Side)

```typescript
// Client A creates a group
async function createGroup(
  apiKey: string,
  creatorClientId: string,
  memberPublicKeys: Map<string, string>, // client_id -> public_key
  groupName: string
) {
  // 1. Generate symmetric group key (32 bytes for AES-256)
  const groupKey = crypto.getRandomValues(new Uint8Array(32));
  
  // 2. Encrypt group key for each member (including creator)
  const encryptedKeys = [];
  
  for (const [clientId, publicKey] of memberPublicKeys) {
    // Use RSA-OAEP or X25519 to encrypt the group key
    const encryptedKey = await encryptGroupKeyForMember(
      groupKey,
      publicKey
    );
    
    encryptedKeys.push({
      client_id: clientId,
      encrypted_key: base64Encode(encryptedKey),
      key_version: 1,
      encrypted_at: new Date().toISOString()
    });
  }
  
  // 3. Create group via API
  const response = await fetch('/api/groups', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-API-Key': apiKey
    },
    body: JSON.stringify({
      api_key_hash: sha256(apiKey),
      creator_client_address: creatorClientId,
      group_name: groupName,
      group_type: 'private',
      encrypted_group_keys: encryptedKeys
    })
  });
  
  const group = await response.json();
  
  // 4. Store group key locally (encrypted with user's master key)
  await storeGroupKeyLocally(group.data.id, groupKey);
  
  return group.data;
}

// Helper: Encrypt group key with member's public key
async function encryptGroupKeyForMember(
  groupKey: Uint8Array,
  memberPublicKeyPem: string
): Promise<ArrayBuffer> {
  // Import public key
  const publicKey = await crypto.subtle.importKey(
    'spki',
    pemToArrayBuffer(memberPublicKeyPem),
    { name: 'RSA-OAEP', hash: 'SHA-256' },
    false,
    ['encrypt']
  );
  
  // Encrypt group key
  const encrypted = await crypto.subtle.encrypt(
    { name: 'RSA-OAEP' },
    publicKey,
    groupKey
  );
  
  return encrypted;
}
```

### 2. Sending Group Message (Client-Side)

```typescript
async function sendGroupMessage(
  apiKey: string,
  groupId: string,
  senderClientId: string,
  plaintext: string
) {
  // 1. Retrieve group key from local storage
  const groupKey = await getGroupKeyLocally(groupId);
  
  if (!groupKey) {
    throw new Error('Group key not found. Fetch from server first.');
  }
  
  // 2. Generate random nonce (12 bytes for AES-GCM)
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  
  // 3. Encrypt message with AES-256-GCM
  const encoder = new TextEncoder();
  const plaintextBytes = encoder.encode(plaintext);
  
  const key = await crypto.subtle.importKey(
    'raw',
    groupKey,
    { name: 'AES-GCM' },
    false,
    ['encrypt']
  );
  
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: nonce },
    key,
    plaintextBytes
  );
  
  // 4. Send to server
  const response = await fetch('/api/messages', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-API-Key': apiKey
    },
    body: JSON.stringify({
      ciphertext: base64Encode(ciphertext),
      nonce: base64Encode(nonce),
      content_type: 'text/plain',
      content_size_bytes: new Uint8Array(ciphertext).length,
      api_key_id: getApiKeyId(apiKey),
      sender_client_id: senderClientId,
      group_id: groupId,
      is_group_message: true
    })
  });
  
  return await response.json();
}
```

### 3. Receiving Group Message (Client-Side)

```typescript
async function receiveGroupMessage(
  apiKey: string,
  messageId: string,
  groupId: string,
  clientId: string,
  clientPrivateKey: string
) {
  // 1. Fetch message from server
  const messageResponse = await fetch(`/api/messages/${messageId}`, {
    headers: { 'X-API-Key': apiKey }
  });
  const message = (await messageResponse.json()).data;
  
  // 2. Check if we have the group key locally
  let groupKey = await getGroupKeyLocally(groupId);
  
  if (!groupKey) {
    // 3. Fetch encrypted group key from server
    const keyResponse = await fetch(
      `/api/groups/${groupId}/key/${clientId}`,
      { headers: { 'X-API-Key': apiKey } }
    );
    const encryptedKeyData = (await keyResponse.json()).data;
    
    // 4. Decrypt group key with our private key
    groupKey = await decryptGroupKey(
      base64Decode(encryptedKeyData.encrypted_key),
      clientPrivateKey
    );
    
    // 5. Store locally for future use
    await storeGroupKeyLocally(groupId, groupKey);
  }
  
  // 6. Decrypt message
  const key = await crypto.subtle.importKey(
    'raw',
    groupKey,
    { name: 'AES-GCM' },
    false,
    ['decrypt']
  );
  
  const plaintext = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: base64Decode(message.nonce) },
    key,
    base64Decode(message.ciphertext)
  );
  
  const decoder = new TextDecoder();
  return decoder.decode(plaintext);
}

// Helper: Decrypt group key with private key
async function decryptGroupKey(
  encryptedKey: ArrayBuffer,
  privateKeyPem: string
): Promise<Uint8Array> {
  const privateKey = await crypto.subtle.importKey(
    'pkcs8',
    pemToArrayBuffer(privateKeyPem),
    { name: 'RSA-OAEP', hash: 'SHA-256' },
    false,
    ['decrypt']
  );
  
  const decrypted = await crypto.subtle.decrypt(
    { name: 'RSA-OAEP' },
    privateKey,
    encryptedKey
  );
  
  return new Uint8Array(decrypted);
}
```

### 4. Adding New Member (Client-Side)

```typescript
async function addMemberToGroup(
  apiKey: string,
  groupId: string,
  newMemberClientId: string,
  newMemberPublicKey: string,
  inviterClientId: string
) {
  // 1. Get current group key
  const groupKey = await getGroupKeyLocally(groupId);
  
  // 2. Encrypt group key for new member
  const encryptedKey = await encryptGroupKeyForMember(
    groupKey,
    newMemberPublicKey
  );
  
  // 3. Get current group to know key_version
  const groupResponse = await fetch(`/api/groups/${groupId}`, {
    headers: { 'X-API-Key': apiKey }
  });
  const group = (await groupResponse.json()).data;
  
  // 4. Add member via API
  const response = await fetch(`/api/groups/${groupId}/members`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-API-Key': apiKey
    },
    body: JSON.stringify({
      api_key_hash: sha256(apiKey),
      client_address: newMemberClientId,
      invited_by: inviterClientId,
      encrypted_group_key: {
        client_id: newMemberClientId,
        encrypted_key: base64Encode(encryptedKey),
        key_version: group.key_version,
        encrypted_at: new Date().toISOString()
      }
    })
  });
  
  return await response.json();
}
```

### 5. Key Rotation (Client-Side - Admin Only)

```typescript
async function rotateGroupKey(
  apiKey: string,
  groupId: string,
  adminClientId: string
) {
  // 1. Get all active members
  const membersResponse = await fetch(`/api/groups/${groupId}/members`, {
    headers: { 'X-API-Key': apiKey }
  });
  const members = (await membersResponse.json()).data;
  
  // 2. Generate NEW group key
  const newGroupKey = crypto.getRandomValues(new Uint8Array(32));
  
  // 3. Encrypt new key for all active members
  const newEncryptedKeys = [];
  
  for (const member of members) {
    if (member.status !== 'active') continue;
    
    // Fetch member's public key
    const clientResponse = await fetch(
      `/api/clients/${member.client_address}`,
      { headers: { 'X-API-Key': apiKey } }
    );
    const client = (await clientResponse.json()).data;
    
    const encryptedKey = await encryptGroupKeyForMember(
      newGroupKey,
      client.public_key
    );
    
    newEncryptedKeys.push({
      client_id: member.client_address,
      encrypted_key: base64Encode(encryptedKey),
      key_version: 0, // Will be incremented by server
      encrypted_at: new Date().toISOString()
    });
  }
  
  // 4. Send key rotation request
  const response = await fetch(`/api/groups/${groupId}/rotate-key`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-API-Key': apiKey
    },
    body: JSON.stringify({
      api_key_hash: sha256(apiKey),
      requester_client: adminClientId,
      new_encrypted_keys: newEncryptedKeys
    })
  });
  
  // 5. Update local storage
  await storeGroupKeyLocally(groupId, newGroupKey);
  
  return await response.json();
}
```

## Server-Side API Endpoints

### 1. Get Encrypted Group Key for Client

```rust
// GET /groups/:group_id/key/:client_id
pub async fn get_encrypted_group_key(
    State(state): State<AppState>,
    Path((group_id, client_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<EncryptedGroupKey> {
    // Verify client is a member of the group
    if !MessageGroup::is_member(&state.db, group_id, client_id).await? {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiResponse {
                data: EncryptedGroupKey::default(),
                message: Some("Not a member of this group".to_string()),
            }),
        ));
    }

    let encrypted_key = MessageGroup::get_encrypted_key_for_client(
        &state.db,
        group_id,
        client_id,
    )
    .await
    .map_err(map_error)?;

    Ok(Json(ApiResponse {
        data: encrypted_key,
        message: None,
    }))
}
```

### 2. Rotate Group Key

```rust
// POST /groups/:group_id/rotate-key
pub async fn rotate_group_key(
    State(state): State<AppState>,
    Path(group_id): Path<Uuid>,
    Json(req): Json<RotateGroupKeyRequest>,
) -> ApiResult<MessageGroup> {
    let group = MessageGroup::rotate_group_key(&state.db, group_id, req)
        .await
        .map_err(map_error)?;

    Ok(Json(ApiResponse {
        data: group,
        message: Some("Group key rotated successfully".to_string()),
    }))
}
```

## Security Considerations

### ✅ What This Provides

1. **End-to-End Encryption**: Server never sees plaintext messages
2. **Forward Secrecy**: Key rotation after members leave
3. **Access Control**: Only group members can decrypt messages
4. **Audit Trail**: Key version tracking

### ⚠️ Limitations

1. **Trust on Add**: New members can't read old messages (unless you implement message re-encryption)
2. **Key Distribution**: Requires secure exchange of public keys
3. **Metadata Visible**: Server knows who's in which group and message timestamps
4. **No Post-Compromise Security**: If device compromised, all messages readable

### 🔒 Best Practices

1. **Rotate keys after every member removal**
2. **Use strong key derivation** (PBKDF2/Argon2) for local key storage
3. **Implement key backup** mechanism for account recovery
4. **Use authenticated encryption** (AES-GCM, not just AES-CBC)
5. **Validate all public keys** before encryption

## Testing Checklist

- [ ] Group creation with encrypted keys
- [ ] Sending encrypted group messages
- [ ] Receiving and decrypting group messages
- [ ] Adding new members with key distribution
- [ ] Key rotation after member removal
- [ ] Error handling for invalid keys
- [ ] Performance with large groups (100+ members)
- [ ] Concurrent message sending
- [ ] Key version mismatch handling

## Performance Optimization

1. **Cache group keys locally** on client devices
2. **Batch encrypt** for multiple recipients
3. **Use database indexes** on group_id and key_version
4. **Lazy load** encrypted keys (only when needed)
5. **Implement pagination** for large groups

## Future Enhancements

1. **Sender Keys Protocol**: For better forward secrecy in large groups
2. **Double Ratchet**: For perfect forward secrecy
3. **Message Reactions**: Encrypted reaction support
4. **File Sharing**: Large file encryption with separate keys
5. **Video/Voice**: Real-time E2EE communication

---

**Next Steps**: Implement the client-side SDK with these functions and test with a small group (3-5 members) before scaling.