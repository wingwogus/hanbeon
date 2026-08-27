'use client'

import { Center, Text } from '@devup-ui/react'

import type { ScanMode } from '@/lib/actions'
import {
  type ArduinoConnection,
  connectionAnnouncement,
  connectionCopy,
  connectionMark,
  connectionSentinel,
  INITIAL_CONNECTION,
} from '@/lib/arduino'
import { formatSeconds } from '@/lib/format'

interface StatusLineProps {
  /** Native switch lifecycle. Waiting until the core emits otherwise. */
  connection?: ArduinoConnection
  /** 현재 주사 간격. 알릴 것이 없을 때 이 값을 보여준다. */
  intervalMs: number
  mode: ScanMode
  /** 적응 로직이 간격을 바꾼 이유. 잠시 떴다가 사라진다. */
  notice: string | null
}

const attentionColor = (connection: ArduinoConnection) => {
  switch (connection.state) {
    case 'error':
      return '$error'
    case 'reconnecting':
    case 'permission':
    case 'action-required':
    case 'suspended':
      return '$warning'
    case 'waiting':
    case 'connecting':
      return '$info'
    default:
      return null
  }
}

/**
 * 컨트롤러 맨 아래 한 줄.
 *
 * 자동으로 바뀐 것은 반드시 보여야 한다(PRD F5, 원칙 2). 사용자는 스위치
 * 타이밍을 몸으로 익히는데, 속도가 소리 없이 바뀌면 갑자기 놓치기 시작하고
 * 왜 그런지 알 방법이 없다.
 *
 * 알릴 것이 없을 때도 줄을 비우지 않고 현재 속도를 보여준다. 나타났다
 * 사라지는 줄은 그때마다 아래 4칸을 밀어 올려, 커서 위치를 다시 찾게 만든다.
 *
 * 스위치 연결 상태도 같은 한 줄에 얹는다. 새 줄을 늘리면 칸이 움직이고,
 * 연결 안내는 속도보다 먼저 읽혀야 한다. 이 줄은 누를 곳이 아니라서
 * 대상 앱의 포커스를 뺏지 않는다.
 *
 * 상태 단서는 색만이 아니라 `data-mark`와 굵기로도 읽힌다(원칙 6).
 */
export function StatusLine({
  connection = INITIAL_CONNECTION,
  intervalMs,
  mode,
  notice,
}: StatusLineProps) {
  const connectionNotice = connectionCopy(connection)
  // Transport loss also pauses the scanner. That halt is not a user pause,
  // so missing-switch copy outranks the long-press pause line.
  //
  // reconnecting deliberately has no connectionCopy so the overlay line does
  // not shift while the cursor runs, but it is still a transport state. Reading
  // it as a user pause would tell the caregiver to long-press to resume while
  // the switch is simply being found again.
  const searching = connection.state === 'reconnecting'
  const userPaused = mode === 'paused' && !connectionNotice && !searching
  const tone = userPaused
    ? '$warning'
    : (attentionColor(connection) ?? (notice ? '$primary' : '$caption'))
  const mark = connectionMark(connection)
  const announcement = userPaused
    ? '일시정지 — 길게 눌러 다시 시작'
    : connectionAnnouncement(connection)

  // 연결 이상 > 사용자 정지 > 최근 조정 > 현재 속도. 스위치가 빠진
  // 멈춤을 일시정지로 읽히게 두면 길게 눌러 다시 시작하려 한다.
  const message = userPaused
    ? '일시정지 — 길게 눌러 다시 시작'
    : (connectionNotice ?? notice ?? `${formatSeconds(intervalMs)}마다`)

  return (
    <Center
      aria-label={announcement}
      data-mark={mark}
      data-state={connectionSentinel(connection)}
      flexShrink={0}
      h="22px"
      overflow="hidden"
      w="100%"
    >
      <Text
        color={tone}
        // 평소 안내와 달라졌다는 것을 색만으로 알리지 않는다(원칙 6).
        fontWeight={userPaused || notice || connectionNotice ? 700 : 400}
        overflow="hidden"
        textOverflow="ellipsis"
        typography="caption"
        whiteSpace="nowrap"
      >
        {message}
      </Text>
    </Center>
  )
}
