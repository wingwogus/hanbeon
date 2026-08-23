import { describe, expect, it } from 'bun:test'
import type { ReactElement } from 'react'

import { StatusLine } from '@/components/StatusLine'

function propsOf(element: ReactElement) {
  return element.props as Record<string, unknown>
}

function messageOf(element: ReactElement) {
  const children = (element.props as { children: ReactElement }).children
  return (children.props as { children: string }).children
}

describe('StatusLine', () => {
  it('알릴 것이 없으면 현재 속도를 보여준다', () => {
    const line = StatusLine({
      intervalMs: 2500,
      mode: 'scanning',
      notice: null,
    })
    expect(propsOf(line)).toHaveProperty('data-state', 'waiting')
    expect(messageOf(line)).toBe('스위치를 연결해 주세요')
  })

  it('간격이 바뀌면 그 이유를 같은 자리에 띄운다', () => {
    const line = StatusLine({
      connection: { state: 'connected', port: 'port-b' },
      intervalMs: 1700,
      mode: 'scanning',
      notice: '최근 반응이 빨라져 1.8초 → 1.7초',
    })
    expect(propsOf(line)).toHaveProperty('data-state', 'connected')
    expect(messageOf(line)).toBe('최근 반응이 빨라져 1.8초 → 1.7초')
  })

  // 멈춰 있다는 사실이 무엇보다 먼저다. 조정 문구에 가려 정지 상태를
  // 놓치면 사용자는 스위치가 고장 난 줄 안다.
  it('정지 중에는 조정 이유보다 정지 안내가 앞선다', () => {
    const line = StatusLine({
      connection: { state: 'connected', port: 'port-b' },
      intervalMs: 1700,
      mode: 'paused',
      notice: '실수가 감지되어 1.8초 → 2.2초',
    })
    expect(propsOf(line)).toHaveProperty('data-state', 'connected')
    expect(messageOf(line)).toBe('일시정지 — 길게 눌러 다시 시작')
  })
})
