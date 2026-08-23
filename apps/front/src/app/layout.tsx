import { resetCss } from '@devup-ui/reset-css'
import type { Metadata } from 'next'

resetCss()

export const metadata: Metadata = {
  title: '한번 (HanBeon)',
  description:
    '한 번의 누름으로 PC를 제어하는 상황적응형 싱글스위치 접근성 소프트웨어',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="ko" suppressHydrationWarning>
      <body>{children}</body>
    </html>
  )
}
