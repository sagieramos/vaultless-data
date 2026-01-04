
"use client";
import { useEffect } from 'react';
import { useRouter } from 'next/navigation';
import { motion } from 'motion/react';

export default function Main() {
  const router = useRouter();

  // Redirect to landing page or dashboard based on auth status
  useEffect(() => {
    // For now, redirect to landing page
    // In a real implementation, you'd check auth status and redirect accordingly
    router.push('/landing');
  }, [router]);

  return (
    <div className="min-h-screen flex items-center justify-center bg-white dark:bg-gray-950">
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.6 }}
        className="text-center"
      >
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600 mx-auto mb-4"></div>
        <p className="text-gray-600 dark:text-gray-400">Loading...</p>
      </motion.div>
    </div>
  );
}
