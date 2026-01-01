/**
 * VaultlessClient - Main client class for secure messaging
 */

import {
  encryptXChaCha20Poly1305,
  decryptXChaCha20Poly1305,
  generateKey,
  signEd25519,
  verifyEd25519,
  generateEd25519KeyPair,
  getPublicKey,
  bytesToBase64,
  base64ToBytes,
} from '../crypto';

import type {
  VaultlessClientConfig,
  SendMessageRequest,
  EncryptedMessage,
  SendMessageOptions,
  SendMessageResponse,
  ClientInfo,
  HandshakeInitiateResponse,
  KeyPair,
  EncryptionResult,
  DecryptionResult,
} from '../types';

import { VaultlessApiClient } from './api';

/**
 * Main Vaultless Client
 */
export class VaultlessClient {
  private api: VaultlessApiClient;
  private signingKeyPair?: KeyPair;
  private encryptionKey?: Uint8Array;

  constructor(config: VaultlessClientConfig) {
    this.api = new VaultlessApiClient(
      config.apiKey,
      config.baseUrl,
      config.timeout
    );
  }

  /**
   * Initialize the client with signing and encryption keys
   * Call this if you want to manage keys client-side
   */
  async initialize(signingPrivateKey?: Uint8Array): Promise<void> {
    // Generate or use provided signing key pair
    if (signingPrivateKey) {
      const publicKey = getPublicKey(signingPrivateKey);
      this.signingKeyPair = {
        privateKey: signingPrivateKey,
        publicKey,
      };
    } else {
      const keyPair = generateEd25519KeyPair();
      this.signingKeyPair = {
        privateKey: keyPair.privateKey,
        publicKey: keyPair.publicKey,
      };
    }

    // Generate encryption key (in production, this should be stored securely)
    this.encryptionKey = await generateKey();
  }

  /**
   * Send a message with automatic encryption
   *
   * @param request - Message request with recipient and content
   * @returns Send message response
   */
  async sendMessage(
    request: SendMessageRequest,
    options: SendMessageOptions = {}
  ): Promise<SendMessageResponse> {
    if (!this.signingKeyPair) {
      throw new Error('Client not initialized. Call initialize() first.');
    }

    // 1. Lookup recipient if using identifier
    let recipientPubkey = request.recipientPubkey;
    let recipientIdentifier = request.recipientIdentifier;

    if (recipientIdentifier && !recipientPubkey) {
      const recipient = await this.api.lookupClient(recipientIdentifier);
      recipientPubkey = recipient.pubkey;
      if (!recipientPubkey) {
        throw new Error('Recipient public key not found');
      }
    } else if (!recipientIdentifier && recipientPubkey) {
      // Try to get identifier from public key
      try {
        const recipient = await this.api.lookupClient(undefined, recipientPubkey);
        recipientIdentifier = recipient.identifier;
      } catch {
        // Identifier not found, continue with just pubkey
      }
    }

    if (!recipientPubkey) {
      throw new Error('Either recipientIdentifier or recipientPubkey must be provided');
    }

    // 2. Encrypt the message
    const encryptionResult = await this.encryptMessage(request.content);

    // 3. Sign the message envelope
    const signature = await this.signMessage({
      recipient_identifier: recipientIdentifier,
      recipient_pubkey: recipientPubkey,
      ciphertext: encryptionResult.ciphertext,
      nonce: encryptionResult.nonce,
      timestamp: new Date().toISOString(),
    });

    // 4. Send via API
    return this.api.sendMessage(
      recipientIdentifier || '',
      recipientPubkey,
      encryptionResult.ciphertext,
      encryptionResult.nonce,
      signature,
      request.sessionId,
      options
    );
  }

  /**
   * Encrypt a message
   *
   * @param plaintext - The message to encrypt
   * @param key - Optional custom encryption key (defaults to client key)
   * @returns Encryption result with ciphertext and nonce
   */
  async encryptMessage(plaintext: string, key?: Uint8Array): Promise<EncryptionResult> {
    const encryptionKey = key || this.encryptionKey;

    if (!encryptionKey) {
      throw new Error('Encryption key not available. Call initialize() first.');
    }

    return encryptXChaCha20Poly1305(plaintext, encryptionKey);
  }

  /**
   * Decrypt a message
   *
   * @param ciphertext - The encrypted message (base64)
   * @param nonce - The nonce used (base64)
   * @param key - Optional custom decryption key (defaults to client key)
   * @returns Decrypted plaintext
   */
  async decryptMessage(
    ciphertext: string,
    nonce: string,
    key?: Uint8Array
  ): Promise<string> {
    const encryptionKey = key || this.encryptionKey;

    if (!encryptionKey) {
      throw new Error('Encryption key not available. Call initialize() first.');
    }

    return decryptXChaCha20Poly1305(ciphertext, nonce, encryptionKey);
  }

  /**
   * Sign a message envelope
   *
   * @param envelope - The message envelope object to sign
   * @returns Base64 encoded signature
   */
  async signMessage(envelope: Record<string, any>): Promise<string> {
    if (!this.signingKeyPair) {
      throw new Error('Signing key not available. Call initialize() first.');
    }

    const jsonString = JSON.stringify(envelope);
    const signature = signEd25519(
      new TextEncoder().encode(jsonString),
      this.signingKeyPair.privateKey
    );

    return bytesToBase64(signature);
  }

  /**
   * Verify a message signature
   *
   * @param envelope - The message envelope object
   * @param signature - Base64 encoded signature
   * @param publicKey - Sender's public key (base64)
   * @returns True if signature is valid
   */
  async verifyMessage(
    envelope: Record<string, any>,
    signature: string,
    publicKey: string
  ): Promise<boolean> {
    const jsonString = JSON.stringify(envelope);
    const signatureBytes = base64ToBytes(signature);
    const publicKeyBytes = base64ToBytes(publicKey);

    return verifyEd25519(
      new TextEncoder().encode(jsonString),
      signatureBytes,
      publicKeyBytes
    );
  }

  /**
   * Get the client's signing public key
   */
  getSigningPublicKey(): Uint8Array | undefined {
    return this.signingKeyPair?.publicKey;
  }

  /**
   * Lookup a client by identifier or public key
   */
  async lookupClient(identifier?: string, pubkey?: string): Promise<ClientInfo> {
    return this.api.lookupClient(identifier, pubkey);
  }

  /**
   * Initiate handshake to establish a secure session
   */
  async initiateHandshake(
    peerIdentifier?: string,
    peerSigningKey?: string
  ): Promise<HandshakeInitiateResponse> {
    return this.api.initiateHandshake(peerIdentifier, peerSigningKey);
  }

  /**
   * Fetch inbox (grouped by sender)
   */
  async fetchInbox() {
    return this.api.fetchInbox();
  }

  /**
   * Fetch messages from a specific sender
   */
  async fetchMessagesBySender(senderPubkey: string, offset: number = 0, limit: number = 20) {
    return this.api.fetchMessagesBySender(senderPubkey, offset, limit);
  }

  /**
   * Mark a message as read
   */
  async markMessageRead(messageId: string) {
    return this.api.markMessageRead(messageId);
  }

  /**
   * Get read receipts for a message
   */
  async getReadReceipts(messageId: string) {
    return this.api.getReadReceipts(messageId);
  }

  /**
   * Get the underlying API client for advanced usage
   */
  getApiClient(): VaultlessApiClient {
    return this.api;
  }
}
