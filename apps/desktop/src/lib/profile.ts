import { invoke } from '@tauri-apps/api/core'

export type UndoMapping = 'back' | 'undo'
export type Theme = 'light' | 'dark' | 'contrast'

/** Rust `profile::Profile`과 필드가 일치해야 한다. */
export interface Profile {
  intervalMs: number
  minIntervalMs: number
  maxIntervalMs: number
  adaptive: boolean
  manualLock: boolean
  longPressMs: number
  switchKey: string
  sound: boolean
  undoMapping: UndoMapping
  theme: Theme
  windowPosition: [number, number] | null
  dimWhenCovered: boolean
  dimPercent: number
  appButtons: boolean
  logging: boolean
  onboarded: boolean
}

/**
 * 저장 결과.
 *
 * 스위치 키 등록만 실패할 수 있어서 경고가 따로 온다. 이때 돌아온 프로필의
 * `switchKey`는 이전 값이므로, 화면은 반드시 이 값으로 다시 맞춰야 한다.
 */
export interface SaveResult {
  profile: Profile
  warning: string | null
}

/** 스위치를 누를 때마다 오는 판정 결과. 스위치 테스트가 쓴다. */
export interface GestureEvent {
  gesture: 'short' | 'long'
  heldMs: number
}

/** 적응 로직이 간격을 바꿨을 때 오는 이유. */
export interface IntervalEvent {
  fromMs: number
  toMs: number
  reason: string
}

export const getProfile = () => invoke<Profile>('get_profile')

export const saveProfile = (next: Profile) =>
  invoke<SaveResult>('save_profile', { next })

export const closeSettings = () => invoke<void>('close_settings')

/** 실증 기록이 쌓이는 폴더. 사용자가 직접 열어 지우거나 건넬 수 있어야 한다. */
export const getLogDirectory = () => invoke<string>('log_directory')
