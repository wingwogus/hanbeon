import { describe, expect, mock, test } from 'bun:test'

const invoke = mock(async (name: string, args?: Record<string, unknown>) => ({
  name,
  args,
}))

mock.module('@tauri-apps/api/core', () => ({
  invoke: (name: string, args?: Record<string, unknown>) => invoke(name, args),
}))

import {
  type ArduinoCandidate,
  asFirmwareState,
  beginFirmwareInstall,
  canBeginInstall,
  cancelFirmwareInstall,
  FIRMWARE_COMMANDS,
  FIRMWARE_COPY,
  FIRMWARE_EVENT,
  firmwareDeviceArgs,
  firmwareErrorText,
  firmwareOwnsPort,
  type FirmwareState,
  firmwareStatusText,
  INITIAL_FIRMWARE_STATE,
  listArduinoCandidates,
  probeArduinoFirmware,
} from '../lib/firmware'

describe('Arduino firmware lifecycle contract', () => {
  test('uses a dedicated Tauri event and idle initial state', () => {
    expect(FIRMWARE_EVENT).toBe('arduino://firmware')
    expect(INITIAL_FIRMWARE_STATE).toEqual({ state: 'idle' })
  })

  test('keeps candidate identity separate from the transient port path', () => {
    const candidate: ArduinoCandidate = {
      deviceId: 'candidate-1',
      displayName: 'Arduino Uno',
      port: '/dev/cu.usbmodem1401',
      vid: 0x2341,
      pid: 0x0043,
    }

    expect(candidate.deviceId).toBe('candidate-1')
    expect(candidate.port).toBe('/dev/cu.usbmodem1401')
  })

  test('renders the three user-facing installation phases', () => {
    const phases: FirmwareState[] = [
      { state: 'preparing', deviceId: 'candidate-1' },
      { state: 'uploading', deviceId: 'candidate-1' },
      { state: 'verifying', deviceId: 'candidate-1' },
    ]

    expect(phases.map(firmwareStatusText)).toEqual([
      'Arduino 준비 중',
      '펌웨어 전송 중',
      '설치 확인 중',
    ])
    expect(phases.every(firmwareOwnsPort)).toBe(true)
  })

  test('uses board-neutral copy while searching', () => {
    expect(firmwareStatusText({ state: 'searching' })).toBe(
      '지원되는 버튼 보드를 USB로 연결해 주세요',
    )
    expect(FIRMWARE_COPY.startTitle).toBe('한번을 버튼 보드와 연결해 볼게요')
    expect(FIRMWARE_COPY.startAction).toBe('시작하기')
  })

  test('distinguishes no response from different firmware', () => {
    expect(
      firmwareStatusText({
        state: 'confirmationRequired',
        deviceId: 'candidate-1',
        reason: 'noResponse',
      }),
    ).toBe('전용 펌웨어가 필요합니다')
    expect(
      firmwareStatusText({
        state: 'confirmationRequired',
        deviceId: 'candidate-1',
        reason: 'differentFirmware',
      }),
    ).toBe('다른 스케치가 설치되어 있습니다')
  })

  test('allows no-response install without a confirmation token', () => {
    expect(
      canBeginInstall(
        {
          state: 'confirmationRequired',
          deviceId: 'candidate-1',
          reason: 'noResponse',
        },
        false,
      ),
    ).toBe(true)
    expect(
      canBeginInstall(
        {
          state: 'confirmationRequired',
          deviceId: 'candidate-1',
          reason: 'differentFirmware',
        },
        false,
      ),
    ).toBe(false)
  })

  test('requires a stronger overwrite acknowledgement for different firmware', () => {
    const state: FirmwareState = {
      state: 'confirmationRequired',
      deviceId: 'candidate-1',
      reason: 'differentFirmware',
      confirmationToken: 'token-1',
    }

    expect(canBeginInstall(state, false)).toBe(false)
    expect(canBeginInstall(state, true)).toBe(true)
  })

  test('maps retryable installer failures without raw port names', () => {
    expect(firmwareErrorText('notFound')).toBe(
      '지원되는 버튼 보드를 찾지 못했습니다',
    )
    expect(firmwareErrorText('portUnavailable')).toBe(
      '포트를 사용할 수 없습니다',
    )
    expect(firmwareErrorText('uploadFailed')).toBe('펌웨어 전송에 실패했습니다')
    expect(firmwareErrorText('verifyFailed')).toBe(
      '설치는 끝났지만 펌웨어 확인에 실패했습니다',
    )
    expect(firmwareErrorText('notFound')).not.toInclude('/dev/')
  })

  test('covers remaining status, payload, and command wrappers', async () => {
    expect(firmwareStatusText({ state: 'idle' })).toBe('')
    expect(
      firmwareStatusText({ state: 'probing', deviceId: 'candidate-1' }),
    ).toBe('기존 펌웨어 확인 중')
    expect(
      firmwareStatusText({ state: 'complete', deviceId: 'candidate-1' }),
    ).toBe('펌웨어 설치가 완료되었습니다')
    expect(firmwareStatusText({ state: 'cancelled' })).toBe(
      '펌웨어 설치를 취소했습니다',
    )
    expect(
      firmwareStatusText({
        state: 'error',
        code: 'notFound',
        retryable: false,
      }),
    ).toBe('지원되는 버튼 보드를 찾지 못했습니다')
    expect(firmwareErrorText('raw stderr')).toBe(
      '펌웨어 설치 중 문제가 발생했습니다',
    )
    expect(asFirmwareState(null)).toBeNull()
    expect(asFirmwareState({ nope: true })).toBeNull()
    expect(asFirmwareState({ state: 1 })).toBeNull()
    expect(asFirmwareState({ state: 'searching' })).toEqual({
      state: 'searching',
    })
    expect(
      // Rust 이벤트가 snake_case로 오던 구버전 호환 경로를 검증한다.
      asFirmwareState({
        state: 'confirmationRequired',
        device_id: 'candidate-1',
        reason: 'noResponse',
      } as Record<string, unknown>),
    ).toEqual({
      state: 'confirmationRequired',
      deviceId: 'candidate-1',
      reason: 'noResponse',
    })
    expect(
      asFirmwareState({
        state: 'confirmationRequired',
        reason: 'noResponse',
      }),
    ).toBeNull()
    await listArduinoCandidates()
    await probeArduinoFirmware('candidate-1')
    await beginFirmwareInstall('candidate-1')
    await cancelFirmwareInstall()
    expect(invoke.mock.calls.map((call) => call[0])).toEqual([
      FIRMWARE_COMMANDS.listCandidates,
      FIRMWARE_COMMANDS.probe,
      FIRMWARE_COMMANDS.beginInstall,
      FIRMWARE_COMMANDS.cancelInstall,
    ])
  })

  test('sends both camelCase and snake_case device ids to Tauri commands', async () => {
    expect(firmwareDeviceArgs('candidate-1')).toEqual({
      deviceId: 'candidate-1',
      device_id: 'candidate-1',
    })
    expect(() => firmwareDeviceArgs('  ')).toThrow('deviceId is required')
    await beginFirmwareInstall('candidate-1')
    const last = invoke.mock.calls.at(-1)
    expect(last?.[0]).toBe(FIRMWARE_COMMANDS.beginInstall)
    expect(last?.[1]).toEqual({
      deviceId: 'candidate-1',
      device_id: 'candidate-1',
    })
  })
})
