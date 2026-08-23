'use client'

import { Center, Text, VStack } from '@devup-ui/react'

/** 이 칸이 지금 어떤 상태인지. */
export type CursorState = 'idle' | 'scanning' | 'dwelling'

interface SwitchButtonProps {
  /** 낮은 칸으로 그릴지. 설정처럼 가장 드물게 쓰는 칸이 쓴다. */
  compact?: boolean
  cursor: CursorState
  /** 한 줄을 통째로 차지할지. 칸이 홀수일 때 마지막 하나가 쓴다. */
  full?: boolean
  label: string
  name: string
  /**
   * 기본 칸인지 앱별 칸인지.
   *
   * 앱별 칸은 지금 앱에서만 쓸 수 있고 앱이 바뀌면 사라진다. 기본 칸과 같아
   * 보이면 사용자는 언제나 있는 칸으로 여기고 자리를 외운다.
   */
  tone?: 'base' | 'extra'
}

/**
 * 스캔 커서가 머무는 칸.
 *
 * 활성 여부는 색만이 아니라 테두리 두께로도 구분한다. 색약 사용자와 고대비
 * 모드에서 색상 대비만으로는 위치를 놓칠 수 있기 때문이다.
 *
 * listbox/option 같은 role은 붙이지 않는다. floating 창은 포커스를 받지 않으므로
 * 화면 낭독기의 탐색 대상이 아니고, 잘못된 semantic을 노출하면 오히려 방해가 된다.
 */
export function SwitchButton({
  compact = false,
  cursor,
  full = false,
  label,
  name,
  tone = 'base',
}: SwitchButtonProps) {
  const active = cursor !== 'idle'
  const extra = tone === 'extra'

  return (
    <VStack
      aria-label={name}
      bg={active ? '$scanCursorBg' : extra ? '$extraIdleBg' : '$scanIdleBg'}
      borderColor={active ? '$scanCursor' : extra ? '$extraBorder' : '$border'}
      // 앱별 칸은 모서리를 덜 둥글게 해 색 없이도 갈래가 읽히게 한다.
      borderRadius={extra ? '8px' : '14px'}
      borderStyle={cursor === 'dwelling' ? 'dashed' : 'solid'}
      borderWidth={active ? '5px' : '2px'}
      boxSizing="border-box"
      color={active ? '$scanCursorText' : '$scanIdleText'}
      flex={1}
      gap="0px"
      gridColumn={full ? 'span 2' : 'auto'}
      justifyContent="center"
      minH={compact ? '48px' : '60px'}
      overflow="hidden"
      px="4px"
    >
      <Center>
        <Text typography="switchLabel">{label}</Text>
      </Center>
      {/* 설명은 커서가 있는 칸에만 단다. 모든 칸에 붙이면 글자가 늘어
          정작 커서가 어디 있는지가 묻힌다.

          머무는 중에는 '한 번 더 누르면 같은 동작'임을 알려야 한다. 이 상태를
          모르면 사용자는 순환을 기다리며 아무것도 하지 않는다. */}
      {active && !compact && (
        <Center>
          <Text lineHeight="1.2" typography="caption">
            {cursor === 'dwelling' ? '한 번 더' : name}
          </Text>
        </Center>
      )}
    </VStack>
  )
}
