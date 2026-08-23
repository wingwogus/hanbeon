import { globalCss, ThemeScript } from '@devup-ui/react'
import { resetCss } from '@devup-ui/reset-css'

import { Provider } from './provider'

resetCss()
globalCss({
  '*': {
    fontFamily: 'Pretendard',
  },
})

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
      <body>
        <Provider>{children}</Provider>
      </body>
    </html>
  )
}
