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
    // 연결 대기·재시도 상태는 소리 없이 진행한다. 오버레이 한 줄이
    // 바뀔 때마다 칸이 밀려 커서 위치를 다시 찾게 만드는 비용이
    // 연결 안내의 가치보다 크다(원칙 2). 실패만은 알려야 한다.
    case 'waiting':
    case 'connecting':
    case 'reconnecting':
      return null
    case 'connected':
      return null
    case 'error':
      return '스위치 연결에 실패했습니다'
  }
}
