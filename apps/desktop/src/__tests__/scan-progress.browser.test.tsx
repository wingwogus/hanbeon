import { describe, expect, it } from 'bun:test'

import { ScanProgress } from '@/components/ScanProgress'

/**
 * 시계에 기대지 않고 눈금을 고정한다.
 *
 * `startedAt`을 미래로 두면 아직 아무것도 흐르지 않은 순간이고, 과거로 두면
 * 마감이 지난 뒤다. 실제 시각과 무관하게 같은 결과가 나와야 테스트가 흔들리지
 * 않는다.
 */
const before = () => performance.now() + 10_000
const after = () => performance.now() - 10_000

describe('ScanProgress', () => {
  it('순환 중에는 남은 시간만큼 막대가 남는다', () => {
    expect(
      <ScanProgress
        mode="scanning"
        phaseMs={2000}
        remainingMs={2000}
        startedAt={before()}
      />,
    ).toMatchSnapshot()
  })

  it('마감이 지나면 막대가 비어 있다', () => {
    expect(
      <ScanProgress
        mode="scanning"
        phaseMs={2000}
        remainingMs={2000}
        startedAt={after()}
      />,
    ).toMatchSnapshot()
  })

  it('머무름은 순환과 다른 색으로 남은 시간을 센다', () => {
    expect(
      <ScanProgress
        mode="dwelling"
        phaseMs={3000}
        remainingMs={3000}
        startedAt={before()}
      />,
    ).toMatchSnapshot()
  })

  // 카운트다운은 사용자를 재촉한다. 되돌리기 창에서는 시간이 지나도 막대가
  // 줄지 않고, '지금 되돌릴 수 있다'는 사실만 남긴다.
  it('되돌리기 창에서는 줄어들지 않는다', () => {
    expect(
      <ScanProgress
        mode="confirm"
        phaseMs={3000}
        remainingMs={3000}
        startedAt={after()}
      />,
    ).toMatchSnapshot()
  })

  it('정지 중에는 진행이 없다', () => {
    expect(
      <ScanProgress
        mode="paused"
        phaseMs={2000}
        remainingMs={2000}
        startedAt={before()}
      />,
    ).toMatchSnapshot()
  })
})
