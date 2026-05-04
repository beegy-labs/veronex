import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  output: 'standalone',
  images: {
    unoptimized: process.env.CI === 'true',
  },
  async headers() {
    return [
      // ── HTML / route data — never cached at edge or browser ────────────────
      //
      // Default Next.js prerender output ships `Cache-Control: s-maxage=31536000`
      // on the HTML, which Cloudflare honors as "cache for 1 year". When a new
      // deploy lands, the old HTML stays at the edge and keeps pointing browsers
      // at old chunk-hash filenames — making every web-side fix invisible until
      // the cache is manually purged.
      //
      // Override to `no-store` for everything that is NOT an immutable
      // content-hashed static asset (those have their own long-cache headers
      // shipped automatically by Next.js and are safe to keep).
      //
      // /_next/static/{chunks,css,media,...} → content-hashed, immutable, 1y cache
      // /_next/image                          → built-in image optimizer
      // /favicon.*                            → public root assets
      // everything else (HTML pages, route data, /api) → no-store, revalidate
      {
        source: '/((?!_next/static|_next/image|favicon).*)',
        headers: [
          { key: 'Cache-Control', value: 'no-store, must-revalidate' },
        ],
      },
    ]
  },
}

export default nextConfig
