'use client'

import { Text, VStack } from '@devup-ui/react'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

import type { GestureEvent } from '@/lib/profile'

/**
 * 스위치 테스트.
 *
 * 임계값을 스스로 맞추려면 '내가 얼마나 눌렀고, 그게 무엇으로 읽혔는지'를
 * 볼 수 있어야 한다. 페르소나는 피로가 쌓이면 짧게 누르려던 입력이 길어지는데,
 * 숫자 없이는 임계값을 얼마나 올려야 할지 알 수 없다.
 */
export function SwitchTester({ longPressMs }: { longPressMs: number }) {
  const [last, setLast] = useState<GestureEvent | null>(null)
  const [count, setCount] = useState(0)

  useEffect(() => {
    const unlisten = listen<GestureEvent>('input://gesture', (event) => {
      setLast(event.payload)
      setCount((previous) => previous + 1)
    })
    return () => {
      unlisten.then((stop) => stop()).catch(() => {})
    }
  }, [])

  const isLong = last?.gesture === 'long'

  return (
    <VStack
      bg={last ? '$primaryBg' : '$grayBg'}
      borderColor={last ? '$primary' : '$borderBold'}
      borderRadius="12px"
      borderStyle={last ? 'solid' : 'dashed'}
      borderWidth="2px"
      gap="6px"
      p="20px"
    >
      {last ? (
        <>
          <Text color="$title" typography="h2">
            {isLong ? '길게 누름' : '짧게 누름'}
          </Text>
          <Text color="$text" typography="bodyL">
            {last.heldMs}밀리초 눌렀습니다 (기준 {longPressMs}밀리초)
          </Text>
          <Text color="$caption" typography="caption">
            {isLong
              ? '이 길이는 취소·일시정지로 읽힙니다.'
              : '이 길이는 선택으로 읽힙니다.'}
          </Text>
          <Text color="$caption" typography="caption">
            지금까지 {count}번 눌렀습니다
          </Text>
        </>
      ) : (
        <>
          <Text color="$text" typography="bodyL">
            스위치를 눌러 보세요
          </Text>
          <Text color="$caption" typography="caption">
            누른 시간과 판정 결과가 여기에 표시됩니다
          </Text>
        </>
      )}
    </VStack>
  )
}
