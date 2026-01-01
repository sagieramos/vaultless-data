# @vaultless/sdk

Secure messaging SDK with XChaCha20-Poly1305 encryption.

## Installation

```bash
npm install @vaultless/sdk
```

or

```bash
yarn add @vaultless/sdk
```

## Quick Start

```javascript
import { VaultlessClient } from '@vaultless/sdk';

// Initialize the client
const client = new VaultlessClient({
  apiKey: process.env.VAULTLESS_API_KEY,
  baseUrl: 'https://api.vaultless.io',
});

// Initialize with keys (generate new ones if not provided)
await client.initialize();

// Send a message (encryption happens automatically)
const response = await client.sendMessage({
  recipientIdentifier: '+1234567890',
  content: 'Hello, secure world!',
});

console.log('Message sent:', response.message_id);
```

## Features

- **End-to-End Encryption**: XChaCha20-Poly1305 encryption
- **Automatic Key Management**: Generate and manage keys client-side
- **Digital Signatures**: Ed25519 signatures for message verification
- **Session Management**: Secure P2P sessions with handshake protocol
- **Type-Safe**: Full TypeScript support

## API Reference

### VaultlessClient

#### Constructor

```typescript
const client = new VaultlessClient({
  apiKey: string;           // Your API key
  baseUrl?: string;          // API base URL (default: https://api.vaultless.io)
  timeout?: number;         // Request timeout in ms (default: 30000)
});
```

#### Methods

##### `initialize(signingPrivateKey?: Uint8Array)`

Initialize the client with signing and encryption keys.

```typescript
await client.initialize();
// or with existing private key
await client.initialize(existingPrivateKey);
```

##### `sendMessage(request: SendMessageRequest, options?: SendMessageOptions)`

Send a message with automatic encryption.

```typescript
const response = await client.sendMessage({
  recipientIdentifier: '+1234567890',  // or use recipientPubkey
  content: 'Hello, secure world!',
  sessionId: 'optional-session-id',
});
```

##### `encryptMessage(plaintext: string, key?: Uint8Array)`

Encrypt a message.

```typescript
const encrypted = await client.encryptMessage('Secret message');
// Returns: { ciphertext: string, nonce: string }
```

##### `decryptMessage(ciphertext: string, nonce: string, key?: Uint8Array)`

Decrypt a message.

```typescript
const decrypted = await client.decryptMessage(
  encrypted.ciphertext,
  encrypted.nonce
);
```

##### `signMessage(envelope: Record<string, any>)`

Sign a message envelope.

```typescript
const signature = await client.signMessage({
  recipient: '+1234567890',
  ciphertext: 'encrypted_data',
  timestamp: new Date().toISOString(),
});
```

##### `verifyMessage(envelope: Record<string, any>, signature: string, publicKey: string)`

Verify a message signature.

```typescript
const isValid = await client.verifyMessage(
  envelope,
  signature,
  senderPublicKey
);
```

##### `lookupClient(identifier?: string, pubkey?: string)`

Lookup a client by identifier or public key.

```typescript
const clientInfo = await client.lookupClient(
  '+1234567890'  // or public key
);
```

##### `initiateHandshake(peerIdentifier?: string, peerSigningKey?: string)`

Initiate a secure session handshake.

```typescript
const response = await client.initiateHandshake('+1234567890');
// Returns: { peer_signing_key: string, peer_identifier?: string }
```

##### `fetchInbox()`

Fetch inbox grouped by sender.

```typescript
const inbox = await client.fetchInbox();
```

##### `fetchMessagesBySender(senderPubkey: string, offset?: number, limit?: number)`

Fetch messages from a specific sender with pagination.

```typescript
const messages = await client.fetchMessagesBySender(
  senderPubkey,
  0,  // offset
  20  // limit
);
```

##### `markMessageRead(messageId: string)`

Mark a message as read.

```typescript
await client.markMessageRead(messageId);
```

##### `getReadReceipts(messageId: string)`

Get read receipts for a message.

```typescript
const receipts = await client.getReadReceipts(messageId);
```

## Cryptographic Functions

### Encryption

```typescript
import {
  encryptXChaCha20Poly1305,
  decryptXChaCha20Poly1305,
  generateKey,
} from '@vaultless/sdk';

// Generate a random 32-byte key
const key = await generateKey();

// Encrypt
const encrypted = await encryptXChaCha20Poly1305('Secret message', key);

// Decrypt
const decrypted = await decryptXChaCha20Poly1305(
  encrypted.ciphertext,
  encrypted.nonce,
  key
);
```

### Signatures

```typescript
import {
  generateEd25519KeyPair,
  signEd25519,
  verifyEd25519,
  getPublicKey,
} from '@vaultless/sdk';

// Generate key pair
const { privateKey, publicKey } = generateEd25519KeyPair();

// Sign
const message = new TextEncoder().encode('Important message');
const signature = signEd25519(message, privateKey);

// Verify
const isValid = verifyEd25519(message, signature, publicKey);
```

## Advanced Usage

### Using Recipient Public Key Directly

```typescript
const response = await client.sendMessage({
  recipientPubkey: 'base64_encoded_public_key',
  content: 'Hello, secure world!',
});
```

### Custom Encryption Key

```typescript
const customKey = await generateKey();
const encrypted = await client.encryptMessage('Secret message', customKey);
```

### Session-Based Encryption

```typescript
const response = await client.sendMessage({
  recipientIdentifier: '+1234567890',
  content: 'Hello, secure world!',
  sessionId: 'custom-session-id',  // Session-specific encryption
});
```

## Security Best Practices

1. **Never share private keys**: Keep signing and encryption keys secure
2. **Use environment variables**: Store API keys in environment variables
3. **Validate input**: Always validate recipient identifiers and public keys
4. **Use HTTPS**: Always use HTTPS for API calls
5. **Key rotation**: Rotate encryption keys periodically in production

## License

MIT
