import { DevupUI } from '@devup-ui/next-plugin'
import type { NextConfig } from 'next'

const nextConfig: NextConfig = {
  // Tauri는 정적 파일을 번들에 담아 서빙하므로 서버 런타임이 없다.
  output: 'export',
  // `out/settings/index.html` 형태로 떨어져야 Tauri 윈도우 url과 맞는다.
  trailingSlash: true,
  images: {
    unoptimized: true,
  },
  experimental: {
    optimizePackageImports: ['@devup-ui/reset-css'],
    // TypeScript 7 has no compiler API yet. Next and typescript-eslint use the
    // official TypeScript 6 compatibility package until the 7.1 API lands.
    useTypeScriptCli: false,
  },
  reactCompiler: true,
}

export default DevupUI(nextConfig)
