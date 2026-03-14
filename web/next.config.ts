import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  output: 'standalone',
  images: {
    unoptimized: process.env.CI === 'true',
  },
}

export default nextConfig
