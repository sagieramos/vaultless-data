import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  async headers() {
    return [
      {
        source: '/:path*',
        headers: [
          {
            key: 'Cache-Control',
            value: 'no-store, must-revalidate',
          },
        ],
      },
    ];
  },
};

// Enable the React compiler (experimental) — cast to any to avoid strict typing errors
/* ;(nextConfig as any).experimental = {
  react: {
    compiler: true,
  },
};
 */
export default nextConfig;
