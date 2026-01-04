"use client";

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useSearchParams } from 'next/navigation';
import { Loader2 } from 'lucide-react';
import { useAuth } from '@/contexts/AuthContext';
import { authApi } from '@/lib/api';
import { toast } from 'sonner';

export default function GoogleCallbackContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { setOAuthTokens } = useAuth();
  const [isProcessing, setIsProcessing] = useState(true);

  useEffect(() => {
    const handleGoogleCallback = async () => {
      const code = searchParams.get('code');
      const state = searchParams.get('state');
      const error = searchParams.get('error');

      try {
        if (error) {
          const errorDescription = searchParams.get('error_description');
          toast.error(errorDescription || 'Google authentication failed');
          router.push('/login');
          return;
        }

        if (!code || !state) {
          toast.error('Invalid OAuth callback parameters');
          router.push('/login');
          return;
        }

        // Handle Google OAuth callback
        const response = await authApi.handleGoogleCallback(code, state);

        // Set OAuth tokens directly
        setOAuthTokens({
          accessToken: response.accessToken,
          refreshToken: response.refreshToken,
          user: response.user,
        });

        toast.success(`Welcome, ${response.user.name || response.user.email}!`);

        // Redirect to dashboard
        const redirectAfter = response.redirectAfter || '/dashboard';
        router.push(redirectAfter);
      } catch (error: any) {
        console.error('Google OAuth callback error:', error);
        toast.error(error.message || 'Failed to complete authentication');
        router.push('/login');
      } finally {
        setIsProcessing(false);
      }
    };

    handleGoogleCallback();
  }, [searchParams, router, setOAuthTokens]);

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-gray-950 flex items-center justify-center">
      <div className="text-center">
        <Loader2 className="w-12 h-12 animate-spin text-blue-600 mx-auto mb-4" />
        <h2 className="text-2xl font-semibold text-gray-900 dark:text-white mb-2">
          {isProcessing ? 'Processing...' : 'Redirecting...'}
        </h2>
        <p className="text-gray-600 dark:text-gray-400">
          Please wait while we complete your authentication
        </p>
      </div>
    </div>
  );
}
