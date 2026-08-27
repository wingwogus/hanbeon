/**
 * Native Arduino / Android transport connection state.
 *
 * The native core owns discovery and reconnect. This module is only the
 * machine-consumed event contract and the copy the status line may show.
 * Port names, MAC addresses, and raw serial/GATT errors stay out of the
 * user-facing text.
 */
export const ARDUINO_EVENT = 'arduino://lifecycle'

export type ArduinoConnection =
  | { state: 'waiting' }
  | { state: 'connecting'; port: string }
  | { state: 'connected'; port: string }
  | { state: 'reconnecting' }
  | { state: 'error'; message: string }
  | { state: 'usb-active' }
  | { state: 'ble-active' }
  | { state: 'permission' }
  | { state: 'action-required' }
  | { state: 'suspended' }

export type TransportStatusCode =
  | 'no-input'
  | 'permission'
  | 'action-required'
  | 'suspended'
  | 'reconnecting'
  | 'paused'
  | 'ready'

export interface TransportStatusSnapshot {
  revision: number
  code: string
  active: string | null
  usb: string
  ble: string
  held: boolean
  suspended: boolean
  paused: boolean
}

/** First paint before the core emits a lifecycle event. */
export const INITIAL_CONNECTION: ArduinoConnection = { state: 'waiting' }

const MAC = /([0-9a-f]{2}:){5}[0-9a-f]{2}/i
const GATT = /gatt|status\s*=?\s*\d+/i
const SERIAL_PATH = /cu\.|tty\.|\/dev\/|COM\d+|usbmodem|serial failed/i

const NATIVE_CODES = new Set<string>([
  'no-input',
  'permission',
  'action-required',
  'suspended',
  'reconnecting',
  'paused',
  'ready',
])

/** Stable sentinel the status UI and tests consume. */
export const connectionSentinel = (connection: ArduinoConnection) =>
  connection.state

/**
 * Non-color status clue. Color may change with the state, but this mark is
 * what the UI and tests use when color is not enough.
 */
export const connectionMark = (connection: ArduinoConnection) => {
  switch (connection.state) {
    case 'usb-active':
    case 'connected':
      return 'usb'
    case 'ble-active':
      return 'ble'
    case 'reconnecting':
      return 'reconnect'
    case 'permission':
      return 'permission'
    case 'action-required':
      return 'action'
    case 'suspended':
      return 'suspend'
    case 'waiting':
      return 'wait'
    case 'connecting':
      return 'connect'
    case 'error':
      return 'error'
  }
}

/**
 * User-facing connection copy.
 *
 * `connected` / active USB or BLE return null so the existing speed/notice
 * line keeps its seat. A changing extra row would shove the four scan cells
 * and force the user to re-find the cursor.
 */
export const connectionCopy = (
  connection: ArduinoConnection,
): string | null => {
  switch (connection.state) {
    case 'waiting':
      return '스위치를 연결해 주세요'
    case 'connecting':
      return '스위치 연결 중'
    case 'connected':
    case 'usb-active':
    case 'ble-active':
      return null
    case 'reconnecting':
      return '스위치 다시 찾는 중'
    case 'error':
      return '스위치 연결에 실패했습니다'
    case 'permission':
      return '블루투스 권한이 필요합니다'
    case 'action-required':
      return '블루투스를 켜 주세요'
    case 'suspended':
      return '스위치가 없어 주사가 멈췄습니다'
  }
}

/** Accessible Korean announcement, including connected USB/BLE. */
export const connectionAnnouncement = (connection: ArduinoConnection) => {
  switch (connection.state) {
    case 'connected':
    case 'usb-active':
      return 'USB 스위치 사용 중'
    case 'ble-active':
      return '블루투스 스위치 사용 중'
    default:
      return connectionCopy(connection) ?? '스위치를 연결해 주세요'
  }
}

export const payloadRevision = (payload: unknown): number | null => {
  if (!payload || typeof payload !== 'object') return null
  const revision = (payload as { revision?: unknown }).revision
  return typeof revision === 'number' && Number.isFinite(revision)
    ? revision
    : null
}

const asRecord = (payload: unknown): Record<string, unknown> | null => {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
    return null
  }
  return payload as Record<string, unknown>
}

const isSafeToken = (value: string) =>
  !MAC.test(value) && !GATT.test(value) && !SERIAL_PATH.test(value)

const nativeConnection = (
  code: string,
  active: string | null,
): ArduinoConnection => {
  switch (code) {
    case 'reconnecting':
      return { state: 'reconnecting' }
    case 'permission':
      return { state: 'permission' }
    case 'action-required':
      return { state: 'action-required' }
    case 'suspended':
      return { state: 'suspended' }
    case 'ready':
    case 'paused':
      if (active === 'usb') return { state: 'usb-active' }
      if (active === 'ble') return { state: 'ble-active' }
      return INITIAL_CONNECTION
    case 'no-input':
    default:
      return INITIAL_CONNECTION
  }
}

const desktopConnection = (
  value: Record<string, unknown>,
): ArduinoConnection => {
  switch (value.state) {
    case 'waiting':
      return { state: 'waiting' }
    case 'reconnecting':
      return { state: 'reconnecting' }
    case 'connecting':
      return { state: 'connecting', port: '' }
    case 'connected':
      return { state: 'connected', port: '' }
    case 'error':
      return { state: 'error', message: '' }
    case 'usb-active':
    case 'ble-active':
    case 'permission':
    case 'action-required':
    case 'suspended':
      return { state: value.state }
    default:
      return INITIAL_CONNECTION
  }
}

/**
 * Accept desktop lifecycle events and the Android sticky snapshot.
 * Unknown or identifier-bearing payloads collapse to a safe waiting state.
 */
export const normalizeArduinoPayload = (
  payload: unknown,
): ArduinoConnection => {
  const value = asRecord(payload)
  if (!value) return INITIAL_CONNECTION

  if (typeof value.state === 'string') {
    return desktopConnection(value)
  }

  if (typeof value.code !== 'string' || !NATIVE_CODES.has(value.code)) {
    return INITIAL_CONNECTION
  }

  const active =
    typeof value.active === 'string' && isSafeToken(value.active)
      ? value.active
      : null
  return nativeConnection(value.code, active)
}
