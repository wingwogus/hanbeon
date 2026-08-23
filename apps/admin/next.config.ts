import { DevupUI } from '@devup-ui/next-plugin'
import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  experimental: {
    optimizePackageImports: ['@devup-ui/reset-css'],
  },
  reactCompiler: true,
}

export default DevupUI(nextConfig)
