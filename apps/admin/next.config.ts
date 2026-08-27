import { DevupUI } from '@devup-ui/next-plugin'
import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  experimental: {
    optimizePackageImports: ['@devup-ui/reset-css'],
    // TypeScript 7 has no compiler API yet. Use Next's CLI bridge through the
    // TypeScript 6 compatibility package until the 7.1 API lands.
    useTypeScriptCli: true,
  },
  reactCompiler: true,
}

export default DevupUI(nextConfig)
