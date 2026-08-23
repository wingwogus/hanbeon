'use client'

import { Center, Text, VStack } from '@devup-ui/react'

import { UNDO_WINDOW_MS } from '@/lib/actions'

/**
 * 되돌리기 창.
 *
 * 이 동안에는 스캔 칸을 아예 감춘다. 4칸을 그대로 두면 '지금 누르면 무슨 일이
 * 일어나는지'가 화면과 어긋나기 때문이다. 남은 시간을 초 단위로 세어 보여주지는
 * 않는다 — 카운트다운은 사용자를 재촉하고, 이 제품의 사용자는 재촉당하면 실수한다.
 * 위쪽 진행 막대도 이 동안에는 줄어들지 않고 가득 찬 채로 있다(`ScanProgress`).
 */
export function UndoPanel() {
  return (
    <VStack
      bg="$undoBg"
      borderColor="$undoText"
      borderRadius="14px"
      borderStyle="solid"
      borderWidth="5px"
      boxSizing="border-box"
      color="$undoText"
      flex={1}
      gap="2px"
      justifyContent="center"
      minH="0"
      overflow="hidden"
      px="8px"
    >
      <Center>
        <Text typography="switchLabel">되돌리기</Text>
      </Center>
      <Center>
        <Text typography="caption">
          {UNDO_WINDOW_MS / 1000}초 안에 누르면 되돌립니다
        </Text>
      </Center>
    </VStack>
  )
}
