import { globalCss, ThemeScript } from '@devup-ui/react'
import { resetCss } from '@devup-ui/reset-css'
import type { Metadata } from 'next'

resetCss()
globalCss({
  '*': {
    fontFamily: 'Pretendard',
  },
})

export const metadata: Metadata = {
  description: '한번 데스크톱 앱이 남긴 실증 기록을 기기 안에서 분석합니다.',
  title: '한번 실증 기록',
}

export default function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ko">
      <head>
        <link
          href="https://cdn.jsdelivr.net/gh/orioncactus/pretendard@v1.3.9/dist/web/static/pretendard.min.css"
          rel="stylesheet"
        />
        <ThemeScript />
      </head>
      <body>{children}</body>
    </html>
  )
}
