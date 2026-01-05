import { ApiException } from '@/types/api';
import type { ApiError } from '@/types/api';

// ============================================================================
// API CLIENT CONFIGURATION
// ============================================================================

const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:8080';

// ============================================================================
// API CLIENT CLASS
// ============================================================================

class ApiClient {
  private baseUrl: string;
  private getAuthToken: () => string | null;

  constructor(
    baseUrl: string,
    getAuthToken: () => string | null = () =>
      typeof window !== 'undefined'
        ? localStorage.getItem('access_token')
        : null
  ) {
    this.baseUrl = baseUrl;
    this.getAuthToken = getAuthToken;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };

    return headers;
  }

  private async handleResponse<T>(response: Response): Promise<T> {
    const contentType = response.headers.get('content-type');

    if (!response.ok) {
      let errorMessage = 'An error occurred';
      let errorCode: string | undefined;
      let errorDetails: any;

      if (contentType?.includes('application/json')) {
        try {
          const error = await response.json();
          if (error && typeof error === 'object' && 'error' in error && typeof error.error === 'object') {
            errorMessage = error.error.message || errorMessage;
            errorCode = error.error.code;
            errorDetails = error.error;
          } else {
            errorMessage = error.message || error.error || error.details || errorMessage;
            errorCode = error.code;
            errorDetails = error.details;
          }
        } catch {
          // If JSON parsing fails, fall back to text.
          try {
            errorMessage = await response.text() || errorMessage;
          } catch {}
        }
      } else {
        try {
          errorMessage = await response.text() || errorMessage;
        } catch {}
      }

      throw new ApiException(errorMessage, response.status, errorCode, errorDetails);
    }

    if (contentType?.includes('application/json')) {
      return response.json();
    }

    return undefined as unknown as T;
  }

  async get<T>(path: string, params?: Record<string, string>, options?: { omitAuth?: boolean }): Promise<T> {
    const url = new URL(`${this.baseUrl}${path}`);
    if (params) {
      Object.entries(params).forEach(([key, value]) => {
        if (value !== undefined && value !== null) {
          url.searchParams.append(key, value);
        }
      });
    }

    const response = await fetch(url.toString(), {
      method: 'GET',
      headers: this.getHeaders(),
      credentials: options?.omitAuth ? undefined : 'include',
    });

    return this.handleResponse<T>(response);
  }

  async post<T>(
    path: string,
    data?: unknown,
    options?: {
      headers?: HeadersInit;
      omitAuth?: boolean;
    }
  ): Promise<T> {
    const headers: HeadersInit = options?.omitAuth
      ? { 'Content-Type': 'application/json' }
      : this.getHeaders();

    if (options?.headers) {
      Object.assign(headers, options.headers);
    }

    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers,
      body: data ? JSON.stringify(data) : undefined,
      credentials: options?.omitAuth ? undefined : 'include',
    });

    return this.handleResponse<T>(response);
  }

  async patch<T>(path: string, data?: unknown): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: data ? JSON.stringify(data) : undefined,
      credentials: 'include',
    });

    return this.handleResponse<T>(response);
  }

  async put<T>(path: string, data?: unknown): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'PUT',
      headers: this.getHeaders(),
      body: data ? JSON.stringify(data) : undefined,
      credentials: 'include',
    });

    return this.handleResponse<T>(response);
  }

  async delete<T>(path: string): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
      credentials: 'include',
    });

    return this.handleResponse<T>(response);
  }

  async upload<T>(path: string, file: File): Promise<T> {
    const formData = new FormData();
    formData.append('file', file);

    const headers: HeadersInit = {};

    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers,
      body: formData,
      credentials: 'include',
    });

    return this.handleResponse<T>(response);
  }
}

// ============================================================================
// EXPORTED CLIENT INSTANCE
// ============================================================================

export const apiClient = new ApiClient(API_BASE_URL);

export { ApiClient };
export default ApiClient;
