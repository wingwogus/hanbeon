import { globalCss, ThemeScript } from '@devup-ui/react'
import { resetCss } from '@devup-ui/reset-css'
import type { Metadata } from 'next'

resetCss()

// `@font-face`는 객체 형태로 넘길 수 없어 원본 CSS로 적는다.
//
// 이 앱은 오프라인에서 동작해야 하므로 CDN 폰트를 쓸 수 없다. 그래서 Pretendard를
// `public/fonts`에 넣어 함께 배포한다.
//
// `font-display: swap` — `block`이면 글꼴을 읽는 동안 글자가 사라진다. 이 화면은
// 사용자가 조작할 수 있는 유일한 통로라 한순간도 비어 있으면 안 된다.
// 값을 쓰지 않는 표현식이지만 이 형태여야 한다. `void`를 붙이면 devup-ui가
// 태그드 템플릿을 알아보지 못해 빌드가 깨진다.
// eslint-disable-next-line no-unused-expressions
globalCss`
  @font-face {
    font-family: 'Pretendard';
    src: url('/fonts/PretendardVariable.woff2') format('woff2-variations');
    font-weight: 45 920;
    font-style: normal;
    font-display: swap;
  }
`

globalCss({
  '*': {
    // 글꼴 이름은 이 한 곳에서만 정한다. 글자 크기 토큰(devup.json)이 각자
    // 글꼴을 들고 있으면 그쪽이 이 규칙을 덮어써서, 대체 글꼴이 통째로 무시된다.
    //
    // 문자열을 상수로 빼지 않는다. devup-ui는 리터럴만 CSS로 뽑아내고, 변수를
    // 넘기면 정의되지 않는 CSS 변수(`var(--f)`)로 바꿔 규칙 자체가 무효가 된다.
    //
    // 뒤의 시스템 글꼴은 폰트 파일을 읽지 못했을 때를 위한 것이다. 대체 글꼴이
    // 없으면 브라우저 기본 명조로 떨어지는데, 작은 크기의 명조는 저시력
    // 사용자에게 특히 불리하다. `Pretendard Variable`은 이 글꼴을 시스템에 직접
    // 설치한 기기에서 잡히는 이름이다.
    fontFamily:
      'Pretendard, "Pretendard Variable", -apple-system, BlinkMacSystemFont, "Apple SD Gothic Neo", "Segoe UI", "Malgun Gothic", system-ui, sans-serif',
  },
  // floating 창은 투명 배경 위에 컨트롤러만 떠 있어야 한다.
  // 배경이 필요한 화면(설정)은 각자 컨테이너에서 칠한다.
  //
  // 여기에 `overflow: hidden`을 두지 않는다. 두 창이 이 규칙을 함께 쓰는데,
  // 설정 화면은 항목이 늘면 창보다 길어져서 아래쪽에 아예 닿을 수 없게 된다.
  // floating 창은 내용이 정확히 창 높이라 넘칠 일이 없고, 넘치더라도 컨트롤러
  // 컨테이너가 자기 자리에서 잘라 낸다.
  'html, body': {
    bg: 'transparent',
    userSelect: 'none',
  },
})

// 모션은 전역 차단이 아니라 '애초에 넣지 않는' 방식으로 없앤다.
// devup-ui의 globalCss는 at-rule 안의 중첩 셀렉터를 지원하지 않아
// `@media (prefers-reduced-motion)` 블록이 깨진다.
// 커서 이동에 트랜지션·애니메이션을 넣지 않는 규칙은 CLAUDE.md에 있다.

export const metadata: Metadata = {
  title: '한번',
  description: '상황적응형 싱글스위치 접근성 도구',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="ko" suppressHydrationWarning>
      <head>
        <ThemeScript />
      </head>
      <body>{children}</body>
    </html>
  )
}
