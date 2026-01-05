import { apiClient } from '../apiClient';
import type {
  RegisterRequest,
  RegisterResponse,
  LoginRequest,
  LoginResponse,
  RefreshTokenRequest,
  RefreshTokenResponse,
  VerifyEmailRequest,
  VerifyEmailResponse,
  RequestPasswordResetRequest,
  RequestPasswordResetResponse,
  ResetPasswordRequest,
  ResetPasswordResponse,
  LogoutResponse,
  UserResponse,
  GoogleAuthInitResponse,
  GoogleAuthResponse,
} from '@/types/api';

// ============================================================================
// AUTHENTICATION API
// ============================================================================

export const authApi = {
  /**
   * Register a new user
   * POST /dev/auth/register
   */
  register: async (data: RegisterRequest): Promise<RegisterResponse> => {
    return apiClient.post<RegisterResponse>('/dev/auth/register', data, { omitAuth: true });
  },

  /**
   * Login with email and password
   * POST /dev/auth/login
   */
  login: async (data: LoginRequest): Promise<LoginResponse> => {
    return apiClient.post<LoginResponse>('/dev/auth/login', data, { omitAuth: true });
  },

  /**
   * Logout current user
   * POST /dev/auth/logout
   */
  logout: async (): Promise<LogoutResponse> => {
    return apiClient.post<LogoutResponse>('/dev/auth/logout');
  },

  /**
   * Refresh access token
   * POST /dev/auth/refresh-token
   */
  refreshToken: async (
    data: RefreshTokenRequest
  ): Promise<RefreshTokenResponse> => {
    return apiClient.post<RefreshTokenResponse>(
      '/dev/auth/refresh-token',
      data,
      { omitAuth: true }
    );
  },

  /**
   * Get current user information
   * GET /dev/auth/me
   */
  getCurrentUser: async (): Promise<UserResponse> => {
    return apiClient.get<UserResponse>('/dev/auth/me');
  },

  // ============================================================================
  // EMAIL VERIFICATION
  // ============================================================================

  /**
   * Verify email using token (POST)
   * POST /dev/auth/verify-email
   */
  verifyEmail: async (
    data: VerifyEmailRequest
  ): Promise<VerifyEmailResponse> => {
    return apiClient.post<VerifyEmailResponse>(
      '/dev/auth/verify-email',
      data,
      { omitAuth: true }
    );
  },

  /**
   * Verify email using token (GET)
   * GET /dev/auth/verify-email
   */
  verifyEmailGet: async (token: string): Promise<VerifyEmailResponse> => {
    return apiClient.get<VerifyEmailResponse>('/dev/auth/verify-email', {
      token,
    }, { omitAuth: true });
  },

  /**
   * Resend verification email
   * POST /dev/auth/resend-verification-email
   */
  resendVerificationEmail: async (
    email: string
  ): Promise<{ message: string; email: string }> => {
    return apiClient.post('/dev/auth/resend-verification-email', { email }, { omitAuth: true });
  },

  // ============================================================================
  // PASSWORD RESET
  // ============================================================================

  /**
   * Request password reset
   * POST /dev/auth/request-password-reset
   */
  requestPasswordReset: async (
    data: RequestPasswordResetRequest
  ): Promise<RequestPasswordResetResponse> => {
    return apiClient.post<RequestPasswordResetResponse>(
      '/dev/auth/request-password-reset',
      data,
      { omitAuth: true }
    );
  },

  /**
   * Reset password with token
   * POST /dev/auth/reset-password
   */
  resetPassword: async (
    data: ResetPasswordRequest
  ): Promise<ResetPasswordResponse> => {
    return apiClient.post<ResetPasswordResponse>(
      '/dev/auth/reset-password',
      data,
      { omitAuth: true }
    );
  },

  // ============================================================================
  // GOOGLE OAUTH
  // ============================================================================

  /**
   * Get Google OAuth authorization URL
   * GET /auth/google/url
   */
  getGoogleAuthUrl: async (
    redirectAfter?: string
  ): Promise<GoogleAuthInitResponse> => {
    if (redirectAfter) {
      return apiClient.get<GoogleAuthInitResponse>('/auth/google/url', {
        redirectAfter,
      }, { omitAuth: true });
    }
    return apiClient.get<GoogleAuthInitResponse>('/auth/google/url', undefined, { omitAuth: true });
  },

  /**
   * Handle Google OAuth callback
   * GET /auth/google/callback
   */
  handleGoogleCallback: async (
    code: string,
    state: string
  ): Promise<GoogleAuthResponse> => {
    return apiClient.get<GoogleAuthResponse>('/auth/google/callback', {
      code,
      state,
    }, { omitAuth: true });
  },
};

export default authApi;
