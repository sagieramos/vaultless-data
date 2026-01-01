/**
 * Vaultless SDK
 *
 * Secure messaging SDK with XChaCha20-Poly1305 encryption
 */

export { VaultlessClient } from './client/vaultless-client';
export { VaultlessApiClient } from './client/api';

export {
  encryptXChaCha20Poly1305,
  decryptXChaCha20Poly1305,
  deriveSharedKey,
  generateKey,
  deriveKeyFromSession,
  generateEd25519KeyPair,
  signEd25519,
  verifyEd25519,
  signObject,
  verifyObjectSignature,
  getPublicKey,
} from './crypto';

export type {
  VaultlessClientConfig,
  SendMessageRequest,
  EncryptedMessage,
  SendMessageOptions,
  SendMessageResponse,
  ClientInfo,
  HandshakeInitiateResponse,
  HandshakeRespondRequest,
  HandshakeCompleteRequest,
  KeyPair,
  EncryptionResult,
  DecryptionResult,
  HandshakeRequestData,
  HandshakeResponseData,
} from './types';
