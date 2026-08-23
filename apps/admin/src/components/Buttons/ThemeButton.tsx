'use client'

import { Center, getTheme, Image, setTheme } from '@devup-ui/react'

export function ThemeButton() {
  return (
    <Center
      _hover={{
        opacity: 0.6,
      }}
      boxSize={6}
      cursor="pointer"
      onClick={() => setTheme(getTheme() === 'dark' ? 'light' : 'dark')}
      pos="relative"
      transition=".2s"
    >
      <Image
        _themeDark={{
          scale: 0,
        }}
        alt="라이트 테마 버튼"
        boxSize={5}
        pos="absolute"
        scale={1}
        src="/icons/light.svg"
        transition=".25s cubic-bezier(0.68, -0.55, 0.27, 1.55);"
      />
      <Image
        _themeDark={{
          scale: 1,
        }}
        alt="다크 테마 버튼"
        boxSize={5}
        pos="absolute"
        scale={0}
        src="/icons/dark.svg"
        transition=".25s cubic-bezier(0.68, -0.55, 0.27, 1.55);"
      />
    </Center>
  )
}
