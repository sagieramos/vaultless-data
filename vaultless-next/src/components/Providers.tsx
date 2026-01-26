"use client";

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ThemeProvider } from 'next-themes';
import { Provider as ReduxProvider } from 'react-redux';
import { store } from '@/store';
import { Toaster } from '../components/ui/sonner';
import { AuthProvider } from '../contexts/AuthContext';
import { ApiException } from '@/types/api';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000, // 5 minutes
      retry: (failureCount, error: ApiException) => {
        if (error.status === 401 || error.status === 403) return false;
        return failureCount < 3;
      },
    },
  },
});

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <ReduxProvider store={store}>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider attribute="class" defaultTheme="light" enableSystem>
          <AuthProvider>
            {children}
            <Toaster />
          </AuthProvider>
        </ThemeProvider>
      </QueryClientProvider>
    </ReduxProvider>
  );
}
