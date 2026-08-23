/**
 * Native Arduino connection state.
 *
 * The Rust core owns discovery and reconnect. This module is only the
 * machine-consumed event contract and the copy the status line may show.
 * Port names and raw serial errors stay out of the user-facing text.
 */
export const ARDUINO_EVENT = 'arduino://lifecycle'

export type ArduinoConnection =
  | { state: 'waiting' }
  | { state: 'connecting'; port: string }
  | { state: 'connected'; port: string }
  | { state: 'reconnecting' }
  | { state: 'error'; message: string }

/** First paint before the core emits a lifecycle event. */
export const INITIAL_CONNECTION: ArduinoConnection = { state: 'waiting' }

/** Stable sentinel the status UI and tests consume. */
export const connectionSentinel = (connection: ArduinoConnection) =>
  connection.state

/**
 * User-facing connection copy.
 *
 * `connected` returns null so the existing speed/notice line keeps its seat.
 * A changing extra row would shove the four scan cells and force the user to
 * re-find the cursor.
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
      return null
    case 'reconnecting':
      return '스위치 다시 찾는 중'
    case 'error':
      return '스위치 연결에 실패했습니다'
  }
}
