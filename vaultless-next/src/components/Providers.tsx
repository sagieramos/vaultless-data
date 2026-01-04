"use client";

import { ThemeProvider } from 'next-themes';
import { Toaster } from '../components/ui/sonner';
import { AuthProvider } from '../contexts/AuthContext';

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <ThemeProvider attribute="class" defaultTheme="light" enableSystem>
      <AuthProvider>
        {children}
        <Toaster />
      </AuthProvider>
    </ThemeProvider>
  );
}
