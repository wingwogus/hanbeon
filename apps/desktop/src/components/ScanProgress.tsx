'use client'

import { Box } from '@devup-ui/react'
import { useEffect, useState } from 'react'

import type { ScanMode } from '@/lib/actions'

/**
 * 남은 시간 눈금을 다시 그리는 주기.
 *
 * 매 프레임 굴리지 않는다. 커서 이동에 모션을 넣지 않는 규칙과 같은 이유로,
 * 쉬지 않고 흐르는 화면은 시선을 막대에 붙잡아 정작 커서를 놓치게 만든다.
 * 60ms면 '줄어들고 있다'를 읽기에 충분하면서 눈이 따라가지 않아도 된다.
 */
const STEP_MS = 60

interface ScanProgressProps {
  mode: ScanMode
  /** 지금 모드가 통째로 지속되는 시간. */
  phaseMs: number
  /** 스냅샷을 받은 시점에 남아 있던 시간. */
  remainingMs: number
  /** 그 스냅샷이 도착한 시각(`performance.now()`). */
  startedAt: number
}

/**
 * 다음 칸으로 넘어가기까지 남은 시간.
 *
 * 스위치 하나로 조작하는 사용자는 '언제 누를지'를 커서가 도착하기 전에
 * 정해야 한다. 남은 시간이 보이지 않으면 매번 커서가 오는 순간에 반응해야
 * 하고, 반응이 늦으면 한 바퀴를 더 기다린다.
 *
 * 되돌리기 창에서는 줄어들지 않는다. 3초를 세어 보여주면 사용자를 재촉하게
 * 되고, 이 제품의 사용자는 재촉당하면 실수한다 — 대신 막대를 가득 채워
 * '지금 되돌릴 수 있다'는 사실만 알린다.
 */
export function ScanProgress({
  mode,
  phaseMs,
  remainingMs,
  startedAt,
}: ScanProgressProps) {
  const counting = mode === 'scanning' || mode === 'dwelling'
  const [percent, setPercent] = useState(100)

  useEffect(() => {
    if (!counting || phaseMs <= 0) return

    const update = () => {
      const left = Math.max(0, remainingMs - (performance.now() - startedAt))
      setPercent(Math.min(100, Math.round((left / phaseMs) * 100)))
    }

    update()
    const timer = setInterval(update, STEP_MS)
    return () => clearInterval(timer)
  }, [counting, phaseMs, remainingMs, startedAt])

  // 정지 중에는 마감이 없고, 되돌리기 창에서는 세지 않는다.
  // 어느 쪽이든 막대를 통째로 없애지는 않는다 — 자리가 사라지면 아래
  // 4칸이 위로 밀려 화면이 흔들리고, 커서 위치를 다시 찾아야 한다.
  let filled = percent
  if (mode === 'paused') filled = 0
  if (mode === 'confirm') filled = 100

  return (
    <Box
      aria-hidden="true"
      bg="$progressTrack"
      borderColor="$border"
      borderRadius="999px"
      borderStyle="solid"
      borderWidth="1px"
      boxSizing="border-box"
      flexShrink={0}
      h="10px"
      overflow="hidden"
      w="100%"
    >
      <Box
        // 되돌리기 색($undoText)은 고대비 테마에서 검정이라 검은 트랙 위에서
        // 사라진다. 세 테마 모두에서 트랙과 대비가 남는 색으로 고른다.
        bg={
          mode === 'confirm'
            ? '$warning'
            : mode === 'dwelling'
              ? '$caption'
              : '$scanCursor'
        }
        h="100%"
        w={`${filled}%`}
      />
    </Box>
  )
}
