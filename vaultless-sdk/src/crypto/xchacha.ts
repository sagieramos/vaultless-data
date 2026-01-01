/**
 * XChaCha20-Poly1305 Encryption Utilities
 */

import { xchacha20poly1305 } from '@noble/ciphers/chacha';

/**
 * Encrypt plaintext using XChaCha20-Poly1305
 *
 * @param plaintext - The message to encrypt (as string)
 * @param key - The encryption key (32 bytes for XChaCha20-Poly1305)
 * @returns Encryption result with ciphertext and nonce
 */
export async function encryptXChaCha20Poly1305(
  plaintext: string,
  key: Uint8Array
): Promise<{ ciphertext: string; nonce: string }> {
  // Ensure key is 32 bytes
  if (key.length !== 32) {
    throw new Error('Key must be 32 bytes for XChaCha20-Poly1305');
  }

  // Generate random 24-byte nonce for XChaCha20
  const nonce = crypto.getRandomValues(new Uint8Array(24));

  // Encrypt
  const cipher = xchacha20poly1305(key, nonce);
  const ciphertext = cipher.encrypt(new TextEncoder().encode(plaintext));

  // Return base64 encoded values
  return {
    ciphertext: bytesToBase64(ciphertext),
    nonce: bytesToBase64(nonce),
  };
}

/**
 * Decrypt ciphertext using XChaCha20-Poly1305
 *
 * @param ciphertext - The encrypted data (base64 encoded)
 * @param nonce - The nonce used for encryption (base64 encoded)
 * @param key - The decryption key (32 bytes)
 * @returns Decrypted plaintext
 */
export async function decryptXChaCha20Poly1305(
  ciphertext: string,
  nonce: string,
  key: Uint8Array
): Promise<string> {
  // Ensure key is 32 bytes
  if (key.length !== 32) {
    throw new Error('Key must be 32 bytes for XChaCha20-Poly1305');
  }

  // Decode base64 values
  const ciphertextBytes = base64ToBytes(ciphertext);
  const nonceBytes = base64ToBytes(nonce);

  // Decrypt
  const cipher = xchacha20poly1305(key, nonceBytes);
  const plaintext = cipher.decrypt(ciphertextBytes);

  return new TextDecoder().decode(plaintext);
}

/**
 * Derive a shared key from two X25519 public keys using Diffie-Hellman
 * This is for future session-based encryption
 *
 * @param privateKey - Your X25519 private key
 * @param peerPublicKey - Peer's X25519 public key
 * @returns 32-byte shared secret
 */
export async function deriveSharedKey(
  privateKey: Uint8Array,
  peerPublicKey: Uint8Array
): Promise<Uint8Array> {
  // Import keys for DH
  const privateJwk = await crypto.subtle.importKey(
    'raw',
    privateKey,
    { name: 'X25519' },
    false,
    ['deriveBits']
  );

  const publicJwk = await crypto.subtle.importKey(
    'raw',
    peerPublicKey,
    { name: 'X25519' },
    false,
    []
  );

  // Derive bits
  const sharedBits = await crypto.subtle.deriveBits(
    {
      name: 'X25519',
      public: publicJwk,
    },
    privateJwk,
    256 // 256 bits = 32 bytes
  );

  return new Uint8Array(sharedBits);
}

/**
 * Generate a random 32-byte key
 */
export async function generateKey(): Promise<Uint8Array> {
  return crypto.getRandomValues(new Uint8Array(32));
}

// Helper functions
function bytesToBase64(bytes: Uint8Array): string {
  const binString = Array.from(bytes, (byte) =>
    String.fromCharCode(byte)
  ).join('');
  return btoa(binString);
}

function base64ToBytes(base64: string): Uint8Array {
  const binString = atob(base64);
  return Uint8Array.from(binString, (m) => m.codePointAt(0)!);
}

/**
 * Generate a deterministic key from a session ID for encryption
 * This is used when session-based encryption is enabled
 */
export async function deriveKeyFromSession(
  sessionId: string,
  salt: Uint8Array
): Promise<Uint8Array> {
  const encoder = new TextEncoder();
  const sessionBytes = encoder.encode(sessionId);

  // Combine session ID with salt
  const combined = new Uint8Array(sessionBytes.length + salt.length);
  combined.set(sessionBytes);
  combined.set(salt, sessionBytes.length);

  // Use SHA-256 to derive a 32-byte key
  const hashBuffer = await crypto.subtle.digest('SHA-256', combined);
  return new Uint8Array(hashBuffer);
}
