/**
 * Ed25519 Digital Signature Utilities
 */

import { ed25519 } from '@noble/curves/ed25519';

/**
 * Generate a new Ed25519 key pair for signing
 *
 * @returns Ed25519 key pair
 */
export function generateEd25519KeyPair(): {
  privateKey: Uint8Array;
  publicKey: Uint8Array;
} {
  const privateKey = ed25519.utils.randomPrivateKey();
  const publicKey = ed25519.getPublicKey(privateKey);

  return { privateKey, publicKey };
}

/**
 * Sign a message using Ed25519
 *
 * @param message - The message to sign (as bytes)
 * @param privateKey - The Ed25519 private key (32 bytes)
 * @returns 64-byte signature
 */
export function signEd25519(
  message: Uint8Array,
  privateKey: Uint8Array
): Uint8Array {
  return ed25519.sign(message, privateKey);
}

/**
 * Verify an Ed25519 signature
 *
 * @param message - The original message (as bytes)
 * @param signature - The signature to verify (64 bytes)
 * @param publicKey - The Ed25519 public key (32 bytes)
 * @returns True if signature is valid
 */
export function verifyEd25519(
  message: Uint8Array,
  signature: Uint8Array,
  publicKey: Uint8Array
): boolean {
  return ed25519.verify(signature, message, publicKey);
}

/**
 * Sign an object (converted to JSON)
 *
 * @param obj - The object to sign
 * @param privateKey - The Ed25519 private key (32 bytes)
 * @returns Base64 encoded signature
 */
export function signObject<T extends Record<string, any>>(
  obj: T,
  privateKey: Uint8Array
): string {
  // Convert object to JSON string
  const jsonString = JSON.stringify(obj);

  // Sign the message
  const signature = signEd25519(
    new TextEncoder().encode(jsonString),
    privateKey
  );

  // Return base64 encoded signature
  return bytesToBase64(signature);
}

/**
 * Verify an object's signature
 *
 * @param obj - The original object
 * @param signature - Base64 encoded signature
 * @param publicKey - The Ed25519 public key (32 bytes)
 * @returns True if signature is valid
 */
export function verifyObjectSignature<T extends Record<string, any>>(
  obj: T,
  signature: string,
  publicKey: Uint8Array
): boolean {
  // Convert object to JSON string
  const jsonString = JSON.stringify(obj);

  // Decode signature
  const signatureBytes = base64ToBytes(signature);

  // Verify
  return verifyEd25519(
    new TextEncoder().encode(jsonString),
    signatureBytes,
    publicKey
  );
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
 * Get public key from private key
 *
 * @param privateKey - The Ed25519 private key (32 bytes)
 * @returns Ed25519 public key (32 bytes)
 */
export function getPublicKey(privateKey: Uint8Array): Uint8Array {
  return ed25519.getPublicKey(privateKey);
}
