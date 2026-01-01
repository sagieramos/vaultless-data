/**
 * Vaultless SDK Types
 */

export interface VaultlessClientConfig {
  apiKey: string;
  baseUrl?: string;
  timeout?: number;
}

export interface SendMessageRequest {
  recipientIdentifier?: string;
  recipientPubkey?: string;
  content: string;
  sessionId?: string;
  requireProofVerification?: boolean;
}

export interface EncryptedMessage {
  ciphertext: string;
  nonce: string;
  signature: string;
  sessionId?: string;
}

export interface SendMessageOptions {
  requireProofVerification?: boolean;
  encryptionAlgorithm?: string;
  algorithmVersion?: number;
}

export interface SendMessageResponse {
  success: boolean;
  message_id: string;
  created_at: string;
  recipient_online: boolean;
}

export interface ClientInfo {
  client_id: string;
  application_id: string;
  pubkey?: string;
  signing_key?: string;
  identifier?: string;
}

export interface HandshakeInitiateResponse {
  peer_signing_key: string;
  peer_identifier?: string;
}

export interface HandshakeRespondRequest {
  handshake_request: HandshakeRequestData;
  session_id: string;
  ephemeral_public_key: string;
  expires_at: string;
}

export interface HandshakeRequestData {
  handshake_id: string;
  signing_pubkey: string;
  ephemeral_exchange_pubkey: string;
  timestamp: string;
  signature: string;
}

export interface HandshakeResponseData {
  handshake_id: string;
  signing_pubkey: string;
  ephemeral_exchange_pubkey: string;
  timestamp: string;
  session_id: string;
  expires_at: string;
  signature: string;
}

export interface HandshakeCompleteRequest {
  handshake_response: HandshakeResponseData;
  expected_handshake_id: string;
  ephemeral_public_key: string;
}

export interface KeyPair {
  privateKey: Uint8Array;
  publicKey: Uint8Array;
}

export interface EncryptionResult {
  ciphertext: string;
  nonce: string;
}

export interface DecryptionResult {
  plaintext: string;
}
