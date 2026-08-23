import { invoke } from '@tauri-apps/api/core'

export const FIRMWARE_EVENT = 'arduino://firmware'

export const FIRMWARE_COMMANDS = {
  listCandidates: 'list_arduino_candidates',
  probe: 'probe_arduino_firmware',
  beginInstall: 'begin_firmware_install',
  cancelInstall: 'cancel_firmware_install',
} as const

export const UNO_VID = 0x2341
export const UNO_PID = 0x0043

export type ArduinoCandidate = {
  deviceId: string
  displayName: string
  port: string
  vid: number
  pid: number
}

export type FirmwareErrorCode =
  | 'notFound'
  | 'portUnavailable'
  | 'uploadFailed'
  | 'verifyFailed'

export type FirmwareState =
  | { state: 'idle' }
  | { state: 'searching' }
  | { state: 'boardFound'; candidates: ArduinoCandidate[] }
  | { state: 'probing'; deviceId: string }
  | { state: 'alreadyInstalled'; deviceId: string }
  | {
      state: 'confirmationRequired'
      deviceId: string
      reason: 'noResponse' | 'differentFirmware'
      confirmationToken?: string
      displayName?: string
    }
  | { state: 'preparing'; deviceId: string }
  | { state: 'uploading'; deviceId: string; progress?: number }
  | { state: 'verifying'; deviceId: string }
  | { state: 'complete'; deviceId: string }
  | { state: 'cancelled' }
  | {
      state: 'error'
      code: string
      retryable: boolean
      detail?: string
    }

export const INITIAL_FIRMWARE_STATE: FirmwareState = { state: 'idle' }

export const FIRMWARE_COPY = {
  startTitle: '한번을 Arduino 버튼과 연결해 볼게요',
  suppliesHeading: '준비물:',
  supplyUno: 'Arduino Uno R3',
  supplyUsb: 'USB 케이블',
  supplyButton: '연결된 아케이드 버튼',
  startAction: '시작하기',
  connect: 'Arduino Uno를 USB로 연결해 주세요',
  found: 'Arduino Uno를 찾았습니다',
  chooseBoard: '연결할 Arduino Uno를 선택해 주세요',
  continue: '다음',
  alreadyInstalled: '전용 펌웨어가 이미 설치되어 있습니다',
  confirmNeed: '버튼을 사용하려면 전용 펌웨어를 설치해야 합니다.',
  confirmReplace: '설치하면 현재 Arduino에 들어 있는 기존 스케치는 교체됩니다.',
  overwriteStrong:
    '다른 프로젝트의 스케치가 들어 있습니다. 설치하면 그 스케치는 사라지며 복구할 수 없습니다.',
  acknowledgeOverwrite: '이 보드의 스케치 교체를 확인합니다',
  install: '펌웨어 설치',
  later: '나중에 하기',
  retry: '다시 시도',
  installing: '설치가 진행 중',
  unoOnly: '이 설치는 공식 Arduino Uno R3만 지원합니다.',
} as const

export const FIRMWARE_ERROR_COPY: Record<FirmwareErrorCode, string> = {
  notFound: 'Arduino를 찾지 못했습니다',
  portUnavailable: '포트를 사용할 수 없습니다',
  uploadFailed: '펌웨어 전송에 실패했습니다',
  verifyFailed: '설치는 끝났지만 펌웨어 확인에 실패했습니다',
}

export function firmwareStatusText(state: FirmwareState): string {
  switch (state.state) {
    case 'idle':
      return ''
    case 'searching':
      return FIRMWARE_COPY.connect
    case 'boardFound':
      return FIRMWARE_COPY.found
    case 'probing':
      return '기존 펌웨어 확인 중'
    case 'alreadyInstalled':
      return FIRMWARE_COPY.alreadyInstalled
    case 'confirmationRequired':
      return state.reason === 'differentFirmware'
        ? '다른 스케치가 설치되어 있습니다'
        : '전용 펌웨어가 필요합니다'
    case 'preparing':
      return 'Arduino 준비 중'
    case 'uploading':
      return '펌웨어 전송 중'
    case 'verifying':
      return '설치 확인 중'
    case 'complete':
      return '펌웨어 설치가 완료되었습니다'
    case 'cancelled':
      return '펌웨어 설치를 취소했습니다'
    case 'error':
      return state.detail
        ? `${firmwareErrorText(state.code)}\n${state.detail}`
        : firmwareErrorText(state.code)
  }
}

export function firmwareErrorText(code: string): string {
  if (code in FIRMWARE_ERROR_COPY) {
    return FIRMWARE_ERROR_COPY[code as FirmwareErrorCode]
  }
  return '펌웨어 설치 중 문제가 발생했습니다'
}

export function firmwareOwnsPort(state: FirmwareState): boolean {
  return (
    state.state === 'preparing' ||
    state.state === 'uploading' ||
    state.state === 'verifying'
  )
}

export function canBeginInstall(
  state: FirmwareState,
  overwriteAcknowledged: boolean,
): boolean {
  if (state.state !== 'confirmationRequired') {
    return false
  }
  if (state.reason === 'differentFirmware' && !overwriteAcknowledged) {
    return false
  }
  return true
}

export function asFirmwareState(payload: unknown): FirmwareState | null {
  if (!payload || typeof payload !== 'object' || !('state' in payload)) {
    return null
  }
  const state = (payload as { state: unknown }).state
  if (typeof state !== 'string') return null
  return payload as FirmwareState
}

export function usbIdentity(vid: number, pid: number): string {
  return `USB ${vid.toString(16).padStart(4, '0')}:${pid.toString(16).padStart(4, '0')}`
}

export function candidateLabel(candidate: ArduinoCandidate): string {
  return `${candidate.displayName}, ${usbIdentity(candidate.vid, candidate.pid)}`
}

export const listArduinoCandidates = () =>
  invoke<ArduinoCandidate[]>(FIRMWARE_COMMANDS.listCandidates)

function firmwareDeviceArgs(deviceId: string) {
  return { deviceId, device_id: deviceId }
}

export const probeArduinoFirmware = (deviceId: string) =>
  invoke<FirmwareState>(FIRMWARE_COMMANDS.probe, firmwareDeviceArgs(deviceId))

export const beginFirmwareInstall = (deviceId: string) =>
  invoke<void>(FIRMWARE_COMMANDS.beginInstall, firmwareDeviceArgs(deviceId))

export const cancelFirmwareInstall = () =>
  invoke<void>(FIRMWARE_COMMANDS.cancelInstall)
