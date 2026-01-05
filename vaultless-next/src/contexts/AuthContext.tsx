'use client';

import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  ReactNode,
} from 'react';
import { toast } from 'sonner';
import Cookies from 'js-cookie';
import { authApi } from '@/lib/api';
import type {
  UserInfo,
  LoginRequest,
  RegisterRequest,
  UserResponse,
  RefreshTokenResponse,
} from '@/types/api';

// ============================================================================
// AUTH CONTEXT TYPES
// ============================================================================

interface AuthProviderProps {
  children: ReactNode;
}

interface AuthContextType {
  // State
  user: UserInfo | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  accessToken: string | null;
  refreshToken: string | null;

  // Actions
  login: (credentials: LoginRequest) => Promise<void>;
  register: (data: RegisterRequest) => Promise<void>;
  logout: () => Promise<void>;
  refreshTokens: () => Promise<RefreshTokenResponse>;
  setOAuthTokens: (tokens: { accessToken: string; refreshToken: string; user: UserInfo }) => void;
  getAccessToken: () => string | null;
  updateUserInfo: () => Promise<void>;
}

// ============================================================================
// AUTH CONTEXT
// ============================================================================

const AuthContext = createContext<AuthContextType | undefined>(undefined);

// ============================================================================
// TOKEN STORAGE KEYS
// ============================================================================

const ACCESS_TOKEN_KEY = 'access_token';
const REFRESH_TOKEN_KEY = 'refresh_token';
const USER_KEY = 'user_info';

// ============================================================================
// AUTH PROVIDER
// ============================================================================

export function AuthProvider({ children }: AuthProviderProps) {
  const [user, setUser] = useState<UserInfo | null>(null);
  const [accessToken, setAccessToken] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // Load user from localStorage on mount (tokens handled by browser cookies)
  useEffect(() => {
    const loadStoredAuth = () => {
      try {
        const storedUser = localStorage.getItem(USER_KEY);

        if (storedUser) {
          const parsedUser = JSON.parse(storedUser);
          setUser(parsedUser);
        }
      } catch (error) {
        console.error('Error loading auth state:', error);
      } finally {
        setIsLoading(false);
      }
    };

    loadStoredAuth();
  }, []);

  // Save user to localStorage
  useEffect(() => {
    if (user) {
      localStorage.setItem(USER_KEY, JSON.stringify(user));
    } else {
      localStorage.removeItem(USER_KEY);
    }
  }, [user]);

  // Login
  const login = useCallback(async (credentials: LoginRequest) => {
    setIsLoading(true);
    try {
      const response = await authApi.login(credentials);

      // Set tokens in cookies instead of localStorage
      // Note: Secure and HttpOnly should ideally be set by the server response
      // But we set them here as client-side fallback/infrastructure
      Cookies.set(ACCESS_TOKEN_KEY, response.accessToken, { secure: true, sameSite: 'strict' });
      Cookies.set(REFRESH_TOKEN_KEY, response.refreshToken, { secure: true, sameSite: 'strict' });

      setAccessToken(response.accessToken);
      setRefreshToken(response.refreshToken);
      setUser(response.user);

      toast.success(`Welcome back, ${response.user.name || response.user.email}!`);
    } catch (error: any) {
      toast.error(error.message || 'Login failed');
      throw error;
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Register
  const register = useCallback(async (data: RegisterRequest) => {
    setIsLoading(true);
    try {
      await authApi.register(data);
      toast.success('Registration successful! Please check your email to verify your account.');
    } catch (error: any) {
      toast.error(error.message || 'Registration failed');
      throw error;
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Logout
  const logout = useCallback(async () => {
    try {
      await authApi.logout();
    } catch (error) {
      console.error('Logout error:', error);
    } finally {
      Cookies.remove(ACCESS_TOKEN_KEY);
      Cookies.remove(REFRESH_TOKEN_KEY);
      setAccessToken(null);
      setRefreshToken(null);
      setUser(null);
      toast.success('Logged out successfully');
    }
  }, []);

  // Refresh tokens
  const refreshTokens = useCallback(async (): Promise<RefreshTokenResponse> => {
    const currentRefreshToken = Cookies.get(REFRESH_TOKEN_KEY);
    if (!currentRefreshToken) {
      throw new Error('No refresh token available');
    }

    try {
      const response = await authApi.refreshToken({
        refreshToken: currentRefreshToken,
      });

      Cookies.set(ACCESS_TOKEN_KEY, response.accessToken, { secure: true, sameSite: 'strict' });
      Cookies.set(REFRESH_TOKEN_KEY, response.refreshToken, { secure: true, sameSite: 'strict' });

      setAccessToken(response.accessToken);
      setRefreshToken(response.refreshToken);

      return response;
    } catch (error) {
      // If refresh fails, logout user
      console.error('Token refresh failed:', error);
      await logout();
      throw error;
    }
  }, [logout]);

  // Set OAuth tokens directly (for Google OAuth callback)
  const setOAuthTokens = useCallback((
    tokens: { accessToken: string; refreshToken: string; user: UserInfo }
  ) => {
    Cookies.set(ACCESS_TOKEN_KEY, tokens.accessToken, { secure: true, sameSite: 'strict' });
    Cookies.set(REFRESH_TOKEN_KEY, tokens.refreshToken, { secure: true, sameSite: 'strict' });

    setAccessToken(tokens.accessToken);
    setRefreshToken(tokens.refreshToken);
    setUser(tokens.user);
  }, []);

  // Get access token
  const getAccessToken = useCallback((): string | null => {
    return accessToken;
  }, [accessToken]);

  // Update user info
  const updateUserInfo = useCallback(async () => {
    try {
      const userData = await authApi.getCurrentUser();
      setUser({
        email: userData.email,
        name: userData.name,
        emailVerified: userData.emailVerified,
        isAdmin: userData.isActive && userData.emailVerified,
      });
    } catch (error) {
      console.error('Failed to fetch user info:', error);
    }
  }, []);

  const value: AuthContextType = {
    // State
    user,
    isAuthenticated: !!user,
    isLoading,
    accessToken,
    refreshToken,

    // Actions
    login,
    register,
    logout,
    refreshTokens,
    setOAuthTokens,
    getAccessToken,
    updateUserInfo,
  };

  return (
    <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
  );
}

// ============================================================================
// AUTH HOOK
// ============================================================================

export function useAuth() {
  const context = useContext(AuthContext);

  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider');
  }

  return context;
}

// ============================================================================
// PROTECTED ROUTE HOOK
// ============================================================================

export function useRequireAuth() {
  const { isAuthenticated, isLoading, user } = useAuth();

  useEffect(() => {
    if (!isLoading && !isAuthenticated) {
      window.location.href = '/login';
    }
  }, [isAuthenticated, isLoading]);

  return { isAuthenticated, isLoading, user };
}
