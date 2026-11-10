import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  // BACKEND_URL (read at build time) makes /api/* on this site forward to the backend,
  // so the public API address stays the same when the backend moves.
  async rewrites() {
    const backend = process.env.BACKEND_URL?.replace(/\/$/, '');
    return backend ? [{ source: '/api/:path*', destination: `${backend}/api/:path*` }] : [];
  },
};

export default nextConfig;
