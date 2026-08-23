import { describe, expect, it } from 'bun:test'

import { SwitchButton } from '@/components/SwitchButton'
import { UndoPanel } from '@/components/UndoPanel'
import { SCAN_ACTIONS } from '@/lib/actions'

describe('SwitchButton', () => {
  it('순환·머무름·비활성을 구분해 렌더한다', () => {
    expect(
      <SwitchButton cursor="scanning" label=">" name="다음으로" />,
    ).toMatchSnapshot()
    expect(
      <SwitchButton cursor="dwelling" label=">" name="다음으로" />,
    ).toMatchSnapshot()
    expect(
      <SwitchButton cursor="idle" label="<" name="이전으로" />,
    ).toMatchSnapshot()
  })

  // 앱별 칸은 지금 앱에서만 쓸 수 있고 앱이 바뀌면 사라진다. 기본 칸과 같아
  // 보이면 사용자는 언제나 있는 칸으로 여기고 자리를 외운다.
  it('앱별 칸은 기본 칸과 다르게 그린다', () => {
    expect(
      <SwitchButton cursor="idle" label="다음 장" name="페이지 넘기기" />,
    ).toMatchSnapshot()
    expect(
      <SwitchButton
        cursor="idle"
        label="다음 장"
        name="페이지 넘기기"
        tone="extra"
      />,
    ).toMatchSnapshot()
  })

  // 설정은 가장 드물게 쓰므로 낮은 칸으로 그린다.
  it('설정 칸은 낮게 그린다', () => {
    expect(
      <SwitchButton compact cursor="scanning" label="설정" name="설정 열기" />,
    ).toMatchSnapshot()
  })

  // 칸이 홀수일 때 마지막 하나가 한 줄을 통째로 쓴다. 빈칸을 남기면
  // 사용자는 거기에도 무언가 있다고 읽는다.
  it('마지막 홀수 칸은 한 줄을 통째로 쓴다', () => {
    expect(
      <SwitchButton cursor="idle" full label="이전 곡" name="이전 곡으로" />,
    ).toMatchSnapshot()
  })
})

describe('UndoPanel', () => {
  it('되돌리기 창을 렌더한다', () => {
    expect(<UndoPanel />).toMatchSnapshot()
  })
})

describe('SCAN_ACTIONS', () => {
  it('스캔 순서는 다음 - 이전 - 선택 - 설정 이다', () => {
    expect(SCAN_ACTIONS.map((action) => action.id)).toEqual([
      'next',
      'prev',
      'enter',
      'settings',
    ])
  })
})
