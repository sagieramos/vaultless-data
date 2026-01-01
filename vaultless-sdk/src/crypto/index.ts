/**
 * Cryptographic utilities
 */

export {
  encryptXChaCha20Poly1305,
  decryptXChaCha20Poly1305,
  deriveSharedKey,
  generateKey,
  deriveKeyFromSession,
} from './xchacha';

export {
  generateEd25519KeyPair,
  signEd25519,
  verifyEd25519,
  signObject,
  verifyObjectSignature,
  getPublicKey,
} from './ed25519';

// Export helper functions
export function bytesToBase64(bytes: Uint8Array): string {
  const binString = Array.from(bytes, (byte) =>
    String.fromCharCode(byte)
  ).join('');
  return btoa(binString);
}

export function base64ToBytes(base64: string): Uint8Array {
  const binString = atob(base64);
  return Uint8Array.from(binString, (m) => m.codePointAt(0)!);
}
