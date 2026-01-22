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
import { apiClient } from '@/lib/apiClient';
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

  // Load auth state from localStorage on mount
  useEffect(() => {
    const loadStoredAuth = () => {
      try {
        const storedUser = localStorage.getItem(USER_KEY);
        const storedAccessToken = localStorage.getItem(ACCESS_TOKEN_KEY);
        const storedRefreshToken = localStorage.getItem(REFRESH_TOKEN_KEY);

        if (storedUser) {
          setUser(JSON.parse(storedUser));
        }
        if (storedAccessToken) {
          setAccessToken(storedAccessToken);
        }
        if (storedRefreshToken) {
          setRefreshToken(storedRefreshToken);
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

      setAccessToken(response.accessToken);
      setRefreshToken(response.refreshToken);
      setUser(response.user);

      // Persist tokens so ApiClient can use them
      try {
        localStorage.setItem(ACCESS_TOKEN_KEY, response.accessToken);
        localStorage.setItem(REFRESH_TOKEN_KEY, response.refreshToken);
      } catch (err) {
        // ignore storage errors
      }

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
      let res = await authApi.register(data);
      toast.success(res.message || 'Registration successful! Please check your email to verify your account.');
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
      try {
        localStorage.removeItem(ACCESS_TOKEN_KEY);
        localStorage.removeItem(REFRESH_TOKEN_KEY);
      } catch (err) {
        // ignore
      }
      setAccessToken(null);
      setRefreshToken(null);
      setUser(null);
      toast.success('Logged out successfully');
    }
  }, []);

  // Refresh tokens
  const refreshTokens = useCallback(async (): Promise<RefreshTokenResponse> => {
    const tokenToRefresh = refreshToken || localStorage.getItem(REFRESH_TOKEN_KEY);
    if (!tokenToRefresh) {
      await logout();
      throw new Error('No refresh token available');
    }

    try {
      const response = await authApi.refreshToken({ refreshToken: tokenToRefresh });

      setAccessToken(response.accessToken);
      setRefreshToken(response.refreshToken);

      // Persist tokens so ApiClient can use them
      try {
        localStorage.setItem(ACCESS_TOKEN_KEY, response.accessToken);
        localStorage.setItem(REFRESH_TOKEN_KEY, response.refreshToken);
      } catch (err) {
        // ignore storage errors
      }

      return response;
    } catch (error) {
      // If refresh fails, logout user
      console.error('Token refresh failed:', error);
      await logout();
      throw error;
    }
  }, [logout, refreshToken]);

  // Set OAuth tokens directly (for Google OAuth callback)
  const setOAuthTokens = useCallback((
    tokens: { accessToken: string; refreshToken: string; user: UserInfo }
  ) => {
    setAccessToken(tokens.accessToken);
    setRefreshToken(tokens.refreshToken);
    setUser(tokens.user);

    // Persist tokens so ApiClient can use them
    try {
      localStorage.setItem(ACCESS_TOKEN_KEY, tokens.accessToken);
      localStorage.setItem(REFRESH_TOKEN_KEY, tokens.refreshToken);
    } catch (err) {
      // ignore storage errors
    }
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

  // Register handler so the AuthContext is notified when apiClient auto-refreshes tokens
  useEffect(() => {
    const handler = async (data: any) => {
      if (data?.accessToken) {
        setAccessToken(data.accessToken);
        try {
          localStorage.setItem(ACCESS_TOKEN_KEY, data.accessToken);
        } catch (e) {
          // ignore
        }
      }

      if (data?.refreshToken) {
        setRefreshToken(data.refreshToken);
        try {
          localStorage.setItem(REFRESH_TOKEN_KEY, data.refreshToken);
        } catch (e) {
          // ignore
        }
      }

      // Optionally re-sync user info after refresh
      try {
        await updateUserInfo();
      } catch (e) {
        // ignore
      }
    };

    try {
      apiClient.setOnTokenRefresh(handler);
    } catch (e) {
      // ignore
    }

    return () => {
      try {
        apiClient.setOnTokenRefresh(undefined);
      } catch (e) {
        // ignore
      }
    };
  }, [updateUserInfo]);

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