/**
 * floating 컨트롤러가 순환시키는 동작. 순서가 곧 스캔 순서다.
 * Rust의 `src-tauri/src/action.rs`와 순서·id가 일치해야 한다.
 */
export const SCAN_ACTIONS = [
  { id: 'next', label: '>', name: '다음으로', hint: 'Tab', kind: 'move' },
  { id: 'prev', label: '<', name: '이전으로', hint: 'Shift+Tab', kind: 'move' },
  { id: 'enter', label: 'Enter', name: '선택', hint: 'Enter', kind: 'enter' },
  {
    id: 'settings',
    label: '설정',
    name: '설정 열기',
    hint: '설정 화면',
    kind: 'settings',
  },
] as const

export type ScanActionId = (typeof SCAN_ACTIONS)[number]['id']

/**
 * 스캔 상태. Rust `scan::Mode`와 일치한다.
 *
 * - `scanning` 커서가 순환 중
 * - `dwelling` 실행한 칸에 머무는 중. 다시 누르면 같은 동작 반복
 * - `confirm`  되돌리기 창. 누르면 직전 선택을 되돌림
 * - `paused`   정지
 */
export type ScanMode = 'scanning' | 'dwelling' | 'confirm' | 'paused'

/**
 * 칸의 갈래. 화면 배치와 생김새가 여기서 갈린다.
 *
 * - `move`     포커스를 옮긴다. 왼쪽에 세로로 쌓인다
 * - `enter`    선택. 오른쪽에 이동 칸 전체 높이로 붙는다
 * - `extra`    앱별 칸. 구분선 아래에 따로 모인다
 * - `settings` 설정 열기. 가장 드물게 쓰므로 맨 아래
 */
export type ScanCellKind = 'move' | 'enter' | 'extra' | 'settings'

/** 화면에 그리는 칸 하나. 코어가 순서대로 보낸다. */
export interface ScanCell {
  label: string
  name: string
  kind: ScanCellKind
}

/** 코어가 `scan://state`로 보내는 커서 상태. */
export interface ScanSnapshot {
  cursor: number
  /**
   * 화면에 그릴 칸 전체.
   *
   * 순환 순서와는 다르다 — `Enter`는 칸이 하나지만 이동 칸마다 뒤에 끼어들어
   * 순환에는 두 번 나온다. `cursor`는 지금 강조할 **칸 번호**다.
   */
  cells: ScanCell[]
  /** 붙어 있는 앱별 프리셋 이름. 없으면 앞 4칸만 돈다. */
  preset: string | null
  mode: ScanMode
  intervalMs: number
  /**
   * 지금 모드가 통째로 지속되는 시간. 남은 시간 표시의 분모다.
   *
   * 모드마다 다르다 — 순환은 주사 간격, 머무름은 그 1.5배, 되돌리기는 3초.
   * 주사 간격만 보고 그리면 머무름·되돌리기에서 눈금이 마감과 어긋난다.
   */
  phaseMs: number
  /** 이 스냅샷을 만든 시점에 남아 있던 시간. */
  remainingMs: number
}

/**
 * 코어가 `window://cover`로 보내는 가려짐 상태.
 *
 * 지금 조작해야 할 요소가 컨트롤러 아래에 들어갔는지 알려준다. 가려진 채로
 * 두면 사용자는 자기가 무엇을 고르고 있는지 볼 수 없는데, 커서는 정상적으로
 * 돌고 있어서 화면만 봐서는 무엇이 잘못됐는지도 알 수 없다.
 */
export interface CoverEvent {
  covered: boolean
  /** 가릴 때 쓸 불투명도(퍼센트). 설정에서 조절한다. */
  percent: number
}

/** 코어가 `scan://error`로 보내는, 사용자가 알아야 하는 문제. */
export interface ScanError {
  message: string
  needsPermission: boolean
}

/** 기본 주사 간격. 사용자 프로필이 있으면 그 값이 우선한다. */
export const DEFAULT_SCAN_INTERVAL_MS = 1800

/** 선택 직후 이 시간 안에 다시 누르면 되돌리기로 판정한다. */
export const UNDO_WINDOW_MS = 3000

/**
 * 간격이 바뀐 이유를 컨트롤러에 띄워 두는 시간.
 *
 * 계속 남겨 두지 않는다 — 그 줄은 평소 현재 속도를 보여주는 자리이고,
 * 지난 조정 문구가 계속 붙어 있으면 지금 값을 확인할 수 없게 된다.
 */
export const INTERVAL_NOTICE_MS = 6000

/**
 * 앱이 바뀌어 칸이 달라졌을 때 코어가 `scan://preset`으로 보내는 안내.
 *
 * 문구는 코어가 만든다. 어떤 프리셋이 몇 칸을 붙였는지는 코어만 알고,
 * 화면이 그 규칙을 한 번 더 갖고 있으면 둘이 어긋난다.
 */
export interface PresetEvent {
  message: string
}
