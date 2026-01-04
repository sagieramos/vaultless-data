import { Suspense } from 'react';
import GoogleCallbackContent from './GoogleCallbackContent';

export default function GoogleCallbackPage() {
  return (
    <Suspense fallback={<div className="min-h-screen flex items-center justify-center">Loading...</div>}>
      <GoogleCallbackContent />
    </Suspense>
  );
}
