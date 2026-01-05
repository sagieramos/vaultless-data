import { ApiException } from '@/types/api';
import type { ApiError } from '@/types/api';

// ============================================================================
// API CLIENT CONFIGURATION
// ============================================================================

const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:8080';
const USE_SECURE_COOKIES = process.env.NEXT_PUBLIC_USE_SECURE_COOKIES?.toLowerCase() === 'true';

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

    // Add authorization header if available
    const accessToken = this.getAuthToken();
    if (accessToken) {
      headers['Authorization'] = `Bearer ${accessToken}`;
    }

    return headers;
  }

  // Attempt to refresh access token (client-only). Returns true on success.
  private async refreshAccessToken(): Promise<boolean> {
    if (typeof window === 'undefined') return false; // only run in browser

    try {
      const response = await fetch(`${this.baseUrl}/dev/auth/refresh-token`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
      });

      if (!response.ok) return false;

      const data = await response.json().catch(() => null);
      if (data && data.accessToken) {
        try {
          localStorage.setItem('access_token', data.accessToken);
        } catch (e) {
          // ignore storage errors
        }
        return true;
      }

      return false;
    } catch (err) {
      return false;
    }
  }

  // Fetch wrapper that retries once after a successful token refresh when encountering 401
  private async fetchWithRefresh(url: string, options: RequestInit, omitAuth?: boolean): Promise<Response> {
    let response = await fetch(url, options);

    if (response.status === 401 && !omitAuth && typeof window !== 'undefined') {
      const refreshed = await this.refreshAccessToken();
      if (refreshed) {
        // rebuild headers so Authorization header (from localStorage via getHeaders) is up to date
        if (!omitAuth) {
          options.headers = this.getHeaders();
        }
        options.credentials = 'include';
        response = await fetch(url, options);
      }
    }

    return response;
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

    const headers = options?.omitAuth ? { 'Content-Type': 'application/json' } : this.getHeaders();

    // Always include credentials so the server can set or receive HttpOnly cookies (login/refresh flows require this)
    const response = await this.fetchWithRefresh(url.toString(), {
      method: 'GET',
      headers,
      credentials: 'include',
    }, options?.omitAuth);

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

    // Always include credentials so the server can set or receive HttpOnly cookies (login/refresh flows require this)
    const response = await this.fetchWithRefresh(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers,
      body: data ? JSON.stringify(data) : undefined,
      credentials: 'include',
    }, options?.omitAuth);

    return this.handleResponse<T>(response);
  }

  async patch<T>(path: string, data?: unknown): Promise<T> {
    const headers = this.getHeaders();

    const response = await this.fetchWithRefresh(`${this.baseUrl}${path}`, {
      method: 'PATCH',
      headers,
      body: data ? JSON.stringify(data) : undefined,
      credentials: 'include',
    });

    return this.handleResponse<T>(response);
  }

  async put<T>(path: string, data?: unknown): Promise<T> {
    const headers = this.getHeaders();

    const response = await this.fetchWithRefresh(`${this.baseUrl}${path}`, {
      method: 'PUT',
      headers,
      body: data ? JSON.stringify(data) : undefined,
      credentials: 'include',
    });

    return this.handleResponse<T>(response);
  }

  async delete<T>(path: string): Promise<T> {
    const headers = this.getHeaders();

    const response = await this.fetchWithRefresh(`${this.baseUrl}${path}`, {
      method: 'DELETE',
      headers,
      credentials: 'include',
    });

    return this.handleResponse<T>(response);
  }

  async upload<T>(path: string, file: File): Promise<T> {
    const formData = new FormData();
    formData.append('file', file);

    const headers: HeadersInit = {};

    const response = await this.fetchWithRefresh(`${this.baseUrl}${path}`, {
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
