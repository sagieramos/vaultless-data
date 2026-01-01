/**
 * API Client for Vaultless API
 */

import type {
  ClientInfo,
  HandshakeInitiateResponse,
  HandshakeRespondRequest,
  HandshakeCompleteRequest,
  SendMessageResponse,
  SendMessageOptions,
} from '../types';

export class VaultlessApiClient {
  private apiKey: string;
  private baseUrl: string;
  private timeout: number;

  constructor(apiKey: string, baseUrl: string = 'https://api.vaultless.io', timeout: number = 30000) {
    this.apiKey = apiKey;
    this.baseUrl = baseUrl.replace(/\/$/, '');
    this.timeout = timeout;
  }

  /**
   * Make an authenticated API request
   */
  private async request<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`;
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    try {
      const response = await fetch(url, {
        ...options,
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${this.apiKey}`,
          ...options.headers,
        },
        signal: controller.signal,
      });

      clearTimeout(timeoutId);

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({
          message: response.statusText,
        }));
        throw new Error((errorData as { message?: string }).message || `API error: ${response.status}`);
      }

      return await response.json() as T;
    } catch (error) {
      clearTimeout(timeoutId);
      throw error;
    }
  }

  /**
   * Lookup a client by identifier or public key
   */
  async lookupClient(identifier?: string, pubkey?: string): Promise<ClientInfo> {
    const params = new URLSearchParams();
    if (identifier) params.append('identifier', identifier);
    if (pubkey) params.append('pubkey', pubkey);

    return this.request<ClientInfo>(`/api/v1/clients/lookup?${params}`);
  }

  /**
   * Initiate handshake to get peer metadata
   */
  async initiateHandshake(
    peerIdentifier?: string,
    peerSigninKey?: string
  ): Promise<HandshakeInitiateResponse> {
    return this.request<HandshakeInitiateResponse>('/api/v1/clients/handshake/initiate', {
      method: 'POST',
      body: JSON.stringify({
        peer_identifier: peerIdentifier,
        peer_signing_key: peerSigninKey,
      }),
    });
  }

  /**
   * Respond to handshake (store responder's session)
   */
  async respondToHandshake(
    request: HandshakeRespondRequest
  ): Promise<{ session_id: string; expires_at: string }> {
    return this.request('/api/v1/clients/handshake/respond', {
      method: 'POST',
      body: JSON.stringify(request),
    });
  }

  /**
   * Complete handshake (store initiator's session)
   */
  async completeHandshake(
    request: HandshakeCompleteRequest
  ): Promise<{ session_id: string; expires_at: string }> {
    return this.request('/api/v1/clients/handshake/complete', {
      method: 'POST',
      body: JSON.stringify(request),
    });
  }

  /**
   * Send an instant message
   */
  async sendMessage(
    recipientIdentifier: string,
    recipientPubkey: string,
    ciphertext: string,
    nonce: string,
    signature: string,
    sessionId?: string,
    options: SendMessageOptions = {}
  ): Promise<SendMessageResponse> {
    return this.request<SendMessageResponse>('/api/messages/send', {
      method: 'POST',
      body: JSON.stringify({
        recipient_identifier: recipientIdentifier,
        recipient_pubkey: recipientPubkey,
        ciphertext,
        nonce,
        signature,
        session_id: sessionId,
        require_proof_verification: options.requireProofVerification ?? true,
        encryption_algorithm: options.encryptionAlgorithm ?? 'xchacha20-poly1305',
        algorithm_version: options.algorithmVersion ?? 1,
      }),
    });
  }

  /**
   * Fetch inbox (grouped by sender)
   */
  async fetchInbox() {
    return this.request('/api/messages/inbox');
  }

  /**
   * Fetch messages from a specific sender
   */
  async fetchMessagesBySender(senderPubkey: string, offset: number = 0, limit: number = 20) {
    return this.request(`/api/messages/sender/${senderPubkey}?offset=${offset}&limit=${limit}`);
  }

  /**
   * Mark a message as read
   */
  async markMessageRead(messageId: string) {
    return this.request(`/api/messages/${messageId}/read`, {
      method: 'POST',
    });
  }

  /**
   * Get read receipts for a message
   */
  async getReadReceipts(messageId: string) {
    return this.request(`/api/messages/${messageId}/receipts`);
  }
}
