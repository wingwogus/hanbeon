import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test'
import { render } from 'bun-test-env-dom'
import { act, type ReactElement, useEffect } from 'react'

import { TrustedSwitchSetup } from '@/components/settings/TrustedSwitchSetup'
import {
  ARDUINO_EVENT,
  type ArduinoConnection,
  connectionCopy,
  connectionMark,
  connectionSentinel,
  INITIAL_CONNECTION,
  normalizeArduinoPayload,
  payloadRevision,
  type TransportStatusSnapshot,
} from '@/lib/arduino'

import {
  tauriEventListeners as listeners,
  tauriEventMockState,
} from './tauri-event.mock'

const SNAPSHOT = {
  cursor: 0,
  cells: [
    { kind: 'move' as const, label: '>', name: '다음으로' },
    { kind: 'enter' as const, label: '선택', name: '선택' },
    { kind: 'extra' as const, label: '보조', name: '보조' },
    { kind: 'settings' as const, label: '설정', name: '설정' },
  ],
  preset: '테스트',
  mode: 'scanning' as const,
  intervalMs: 1800,
  phaseMs: 1800,
  remainingMs: 1800,
}

const LIFECYCLE: ArduinoConnection[] = [
  { state: 'waiting' },
  { state: 'connecting', port: 'port-a' },
  { state: 'connected', port: 'port-b' },
  { state: 'reconnecting' },
  { state: 'error', message: 'serial failed' },
]

const SCANNING = {
  intervalMs: 1800,
  mode: 'scanning' as const,
  notice: null,
}

const EMPTY_TRANSPORT: TransportStatusSnapshot = {
  revision: 0,
  code: 'no-input',
  active: null,
  usb: 'stopped',
  ble: 'stopped',
  held: false,
  suspended: false,
  paused: false,
}

const TRANSPORT_SNAPSHOTS: TransportStatusSnapshot[] = [
  {
    ...EMPTY_TRANSPORT,
    revision: 3,
    code: 'ready',
    active: 'usb',
    usb: 'ready',
  },
  {
    ...EMPTY_TRANSPORT,
    revision: 4,
    code: 'ready',
    active: 'ble',
    ble: 'ready',
  },
  {
    ...EMPTY_TRANSPORT,
    revision: 5,
    code: 'reconnecting',
    usb: 'starting',
  },
  {
    ...EMPTY_TRANSPORT,
    revision: 6,
    code: 'permission',
  },
  {
    ...EMPTY_TRANSPORT,
    revision: 7,
    code: 'action-required',
  },
  {
    ...EMPTY_TRANSPORT,
    revision: 8,
    code: 'suspended',
    suspended: true,
  },
]

const invokeCalls: string[] = []
const focus = mock(() => {})
const originalSetTimeout = globalThis.setTimeout
const originalSetInterval = globalThis.setInterval
let intervalTicks: (() => void) | undefined
let invokeResult: Promise<unknown> = Promise.resolve(SNAPSHOT)
let transportSnapshotResult: Promise<unknown> = Promise.resolve(EMPTY_TRANSPORT)
let profileResult: Promise<unknown> = Promise.resolve({ onboarded: true })
let overlayResult: Promise<unknown> = Promise.resolve()

mock.module('@tauri-apps/api/core', () => ({
  SERIALIZE_TO_IPC_FN: '__TAURI_TO_IPC_KEY__',
  invoke: (command?: string) => {
    if (command) invokeCalls.push(command)
    if (command === 'transport_status_snapshot') return transportSnapshotResult
    if (command === 'get_profile') return profileResult
    if (command === 'start_overlay_service') return overlayResult
    return invokeResult
  },
}))

mock.module('@/components/DragHandle', () => ({
  DragHandle: () => <div aria-hidden="true" />,
}))

mock.module('@/components/settings/Onboarding', () => ({
  Onboarding: ({ onDone }: { onDone?: (profile: unknown) => void }) => {
    useEffect(() => {
      onDone?.({ onboarded: true })
    }, [onDone])
    return (
      <>
        <span>스위치를 눌러 보세요</span>
        <TrustedSwitchSetup />
      </>
    )
  },
}))

mock.module('@/components/settings/SettingsForm', () => ({
  SettingsForm: ({ onClose }: { onClose?: () => void }) => {
    useEffect(() => {
      onClose?.()
    }, [onClose])
    return (
      <>
        <span>스위치 키</span>
        <TrustedSwitchSetup />
      </>
    )
  },
}))

mock.module('@/lib/profile', () => ({
  getProfile: () => profileResult,
}))

function emit(event: string, payload: unknown) {
  const listener = listeners.get(event)
  if (!listener) throw new Error(`No listener registered for ${event}`)
  listener({ payload })
}

function propsOf(element: ReactElement) {
  return element.props as Record<string, unknown>
}

function scanControl(container: Element) {
  const control = container.querySelector('[aria-label="다음으로"]')
  if (!control) throw new Error('Next scan control did not render')
  return control
}

function messageOf(element: ReactElement) {
  const children = (element.props as { children: ReactElement }).children
  return (children.props as { children: string }).children
}

function statusState(container: Element) {
  return container.querySelector('[data-state]')?.getAttribute('data-state')
}

function statusMark(container: Element) {
  return container.querySelector('[data-mark]')?.getAttribute('data-mark')
}

function statusCopy(container: Element) {
  const line = container.querySelector('[data-state]')
  return `${line?.textContent ?? ''}${line?.getAttribute('aria-label') ?? ''}`
}

describe('Arduino connection status', () => {
  it.each(LIFECYCLE)(
    'exposes machine-consumed $state sentinel',
    (connection) => {
      expect(connectionSentinel(connection)).toBe(connection.state)
      expect(ARDUINO_EVENT).toBe('arduino://lifecycle')
      expect(JSON.stringify(connectionCopy(connection) ?? '')).not.toInclude(
        'port-',
      )
      expect(JSON.stringify(connectionCopy(connection) ?? '')).not.toInclude(
        'serial failed',
      )
    },
  )

  it('connected state sentinel renders', async () => {
    const { StatusLine } = await import('@/components/StatusLine')
    const connection = { state: 'connected', port: 'port-b' } as const
    expect(connectionSentinel(connection)).toBe('connected')
    expect(propsOf(StatusLine({ ...SCANNING, connection }))).toHaveProperty(
      'data-state',
      'connected',
    )
  })

  it('reconnecting sentinel renders while scan controls remain enabled', async () => {
    const [{ StatusLine }, { SwitchButton }] = await Promise.all([
      import('@/components/StatusLine'),
      import('@/components/SwitchButton'),
    ])
    const status = propsOf(
      StatusLine({ ...SCANNING, connection: { state: 'reconnecting' } }),
    )
    const scan = propsOf(
      SwitchButton({ cursor: 'scanning', label: '>', name: '다음으로' }),
    )

    expect(connectionSentinel({ state: 'reconnecting' })).toBe('reconnecting')
    expect(status).toHaveProperty('data-state', 'reconnecting')
    expect(scan).toHaveProperty('aria-label', '다음으로')
    expect(scan).not.toHaveProperty('disabled')
    expect(scan).not.toHaveProperty('aria-disabled')
  })

  it('replaces a stale connected sentinel on reconnect', async () => {
    const { StatusLine } = await import('@/components/StatusLine')
    expect(
      propsOf(
        StatusLine({
          ...SCANNING,
          connection: { state: 'connected', port: 'old-port' },
        }),
      ),
    ).toHaveProperty('data-state', 'connected')
    expect(
      propsOf(
        StatusLine({ ...SCANNING, connection: { state: 'reconnecting' } }),
      ),
    ).toHaveProperty('data-state', 'reconnecting')
  })

  it('does not surface port identity or raw error text as the sentinel', async () => {
    const { StatusLine } = await import('@/components/StatusLine')
    const connected = propsOf(
      StatusLine({
        ...SCANNING,
        connection: { state: 'connected', port: 'cu.usbmodem21401' },
      }),
    )
    const failed = propsOf(
      StatusLine({
        ...SCANNING,
        connection: {
          state: 'error',
          message: 'No such file or directory',
        },
      }),
    )

    expect(connected).toHaveProperty('data-state', 'connected')
    expect(JSON.stringify(connected)).not.toInclude('cu.usbmodem21401')
    expect(failed).toHaveProperty('data-state', 'error')
    expect(JSON.stringify(failed)).not.toInclude('No such file or directory')
    expect(
      connectionCopy({
        state: 'error',
        message: 'No such file or directory',
      }),
    ).not.toInclude('No such file or directory')
  })

  it('defaults to waiting without disabling scan controls', async () => {
    const [{ StatusLine }, { SwitchButton }] = await Promise.all([
      import('@/components/StatusLine'),
      import('@/components/SwitchButton'),
    ])
    const status = propsOf(StatusLine({ ...SCANNING }))
    const scan = propsOf(
      SwitchButton({ cursor: 'scanning', label: '>', name: '다음으로' }),
    )

    expect(connectionSentinel(INITIAL_CONNECTION)).toBe('waiting')
    expect(status).toHaveProperty('data-state', 'waiting')
    expect(scan).not.toHaveProperty('disabled')
    expect(scan).not.toHaveProperty('aria-disabled')
  })

  it.each([
    ['ready', 'usb', 'usb-active', 'USB', 'usb'],
    ['ready', 'ble', 'ble-active', '블루투스', 'ble'],
    ['reconnecting', null, 'reconnecting', '다시', 'reconnect'],
    ['permission', null, 'permission', '권한', 'permission'],
    ['action-required', null, 'action-required', '블루투스', 'action'],
    ['suspended', null, 'suspended', '멈', 'suspend'],
  ] as const)(
    'maps native %s/%s snapshot to %s with Korean copy and a non-color mark',
    async (code, active, state, copy, mark) => {
      const { StatusLine } = await import('@/components/StatusLine')
      const snapshot: TransportStatusSnapshot = {
        ...EMPTY_TRANSPORT,
        revision: 11,
        code,
        active,
        usb: active === 'usb' ? 'ready' : 'stopped',
        ble: active === 'ble' ? 'ready' : 'stopped',
        suspended: code === 'suspended',
      }
      const connection = normalizeArduinoPayload(snapshot)
      const status = propsOf(StatusLine({ ...SCANNING, connection }))
      const visible = connectionCopy(connection) ?? ''
      const rendered = JSON.stringify(status)

      expect(connectionSentinel(connection)).toBe(state)
      expect(connectionMark(connection)).toBe(mark)
      expect(status).toHaveProperty('data-state', state)
      expect(status).toHaveProperty('data-mark', mark)
      expect(String(status['aria-label'] ?? '')).toInclude(copy)
      expect(`${visible}${String(status['aria-label'] ?? '')}`).toInclude(copy)
      expect(rendered).not.toInclude('port-')
      expect(rendered).not.toInclude('AA:BB')
      expect(messageOf(StatusLine({ ...SCANNING, connection }))).not.toInclude(
        'GATT',
      )
    },
  )

  it('redacts raw identifiers from a malformed native status payload', async () => {
    const { StatusLine } = await import('@/components/StatusLine')
    const connection = normalizeArduinoPayload({
      revision: 12,
      code: 'ready',
      active: 'AA:BB:CC:DD:EE:FF',
      usb: '/dev/cu.usbmodem21401',
      ble: 'GATT error 133',
      held: false,
      suspended: false,
      paused: false,
      message: 'serial failed on cu.usbmodem21401',
    })
    const status = propsOf(StatusLine({ ...SCANNING, connection }))
    const copy = `${connectionCopy(connection) ?? ''}${String(status['aria-label'] ?? '')}`

    expect(JSON.stringify(status)).not.toInclude('AA:BB:CC:DD:EE:FF')
    expect(JSON.stringify(status)).not.toInclude('cu.usbmodem21401')
    expect(JSON.stringify(status)).not.toInclude('GATT')
    expect(copy).not.toInclude('AA:BB:CC:DD:EE:FF')
    expect(copy).not.toInclude('cu.usbmodem21401')
    expect(copy).not.toInclude('GATT')
    expect(connectionSentinel(connection)).not.toBe('AA:BB:CC:DD:EE:FF')
  })

  it('falls back to waiting when the native payload is malformed', () => {
    expect(connectionSentinel(normalizeArduinoPayload(null))).toBe('waiting')
    expect(connectionSentinel(normalizeArduinoPayload([]))).toBe('waiting')
    expect(connectionSentinel(normalizeArduinoPayload({}))).toBe('waiting')
    expect(
      connectionSentinel(normalizeArduinoPayload({ code: 'unknown-code' })),
    ).toBe('waiting')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'not-a-state' })),
    ).toBe('waiting')
    expect(
      connectionSentinel(
        normalizeArduinoPayload({ code: 'ready', active: 'wifi' }),
      ),
    ).toBe('waiting')
    expect(
      connectionSentinel(
        normalizeArduinoPayload({ code: 'paused', active: 'usb' }),
      ),
    ).toBe('usb-active')
    expect(
      connectionSentinel(
        normalizeArduinoPayload({ code: 'paused', active: 'ble' }),
      ),
    ).toBe('ble-active')
    expect(
      connectionSentinel(
        normalizeArduinoPayload({ code: 'paused', active: null }),
      ),
    ).toBe('waiting')
    expect(
      connectionSentinel(
        normalizeArduinoPayload({ code: 'no-input', active: null }),
      ),
    ).toBe('waiting')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'waiting' })),
    ).toBe('waiting')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'reconnecting' })),
    ).toBe('reconnecting')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'usb-active' })),
    ).toBe('usb-active')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'ble-active' })),
    ).toBe('ble-active')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'permission' })),
    ).toBe('permission')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'action-required' })),
    ).toBe('action-required')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'suspended' })),
    ).toBe('suspended')
    expect(
      connectionSentinel(normalizeArduinoPayload({ state: 'reconnecting' })),
    ).toBe('reconnecting')
    expect(
      connectionSentinel(
        normalizeArduinoPayload({ state: 'connecting', port: 'port-a' }),
      ),
    ).toBe('connecting')
    expect(
      connectionSentinel(
        normalizeArduinoPayload({
          state: 'error',
          message: 'serial failed',
        }),
      ),
    ).toBe('error')
    expect(connectionMark({ state: 'waiting' })).toBe('wait')
    expect(connectionMark({ state: 'connecting', port: '' })).toBe('connect')
    expect(connectionMark({ state: 'error', message: '' })).toBe('error')
    expect(payloadRevision(null)).toBeNull()
    expect(payloadRevision({ revision: '1' })).toBeNull()
    expect(payloadRevision({ revision: 4 })).toBe(4)
  })

  it('maps native paused code to the active transport, not suspension', () => {
    const pausedUsb = normalizeArduinoPayload({
      ...EMPTY_TRANSPORT,
      revision: 13,
      code: 'paused',
      active: 'usb',
      usb: 'ready',
      paused: true,
    })
    const pausedBle = normalizeArduinoPayload({
      ...EMPTY_TRANSPORT,
      revision: 14,
      code: 'paused',
      active: 'ble',
      ble: 'ready',
      paused: true,
    })
    const suspended = normalizeArduinoPayload({
      ...EMPTY_TRANSPORT,
      revision: 15,
      code: 'suspended',
      suspended: true,
    })

    expect(connectionSentinel(pausedUsb)).toBe('usb-active')
    expect(connectionSentinel(pausedBle)).toBe('ble-active')
    expect(connectionSentinel(suspended)).toBe('suspended')
    expect(connectionCopy(pausedUsb)).toBeNull()
    expect(connectionCopy(pausedBle)).toBeNull()
    expect(connectionCopy(suspended)).toBe('스위치가 없어 주사가 멈췄습니다')
    expect(connectionCopy(suspended)).not.toInclude('일시정지')
  })

  it('renders user pause and transport suspension as distinct Korean copy', async () => {
    const { StatusLine } = await import('@/components/StatusLine')
    const notice = '실수가 감지되어 1.8초 → 2.2초'
    const userPause = StatusLine({
      connection: { state: 'usb-active' },
      intervalMs: 1700,
      mode: 'paused',
      notice,
    })
    const blePause = StatusLine({
      connection: { state: 'ble-active' },
      intervalMs: 1700,
      mode: 'paused',
      notice,
    })
    const suspended = StatusLine({
      connection: { state: 'suspended' },
      intervalMs: 1700,
      mode: 'paused',
      notice,
    })
    const reconnecting = StatusLine({
      connection: { state: 'reconnecting' },
      intervalMs: 1700,
      mode: 'paused',
      notice,
    })
    const permission = StatusLine({
      connection: { state: 'permission' },
      intervalMs: 1700,
      mode: 'paused',
      notice,
    })
    const action = StatusLine({
      connection: { state: 'action-required' },
      intervalMs: 1700,
      mode: 'paused',
      notice,
    })

    expect(propsOf(userPause)).toHaveProperty('data-state', 'usb-active')
    expect(propsOf(userPause)).toHaveProperty('data-mark', 'usb')
    expect(messageOf(userPause)).toBe('일시정지 — 길게 눌러 다시 시작')
    expect(String(propsOf(userPause)['aria-label'])).toInclude('일시정지')
    expect(messageOf(userPause)).not.toInclude('스위치가 없어')

    expect(propsOf(blePause)).toHaveProperty('data-state', 'ble-active')
    expect(propsOf(blePause)).toHaveProperty('data-mark', 'ble')
    expect(messageOf(blePause)).toBe('일시정지 — 길게 눌러 다시 시작')

    expect(propsOf(suspended)).toHaveProperty('data-state', 'suspended')
    expect(propsOf(suspended)).toHaveProperty('data-mark', 'suspend')
    expect(messageOf(suspended)).toBe('스위치가 없어 주사가 멈췄습니다')
    expect(String(propsOf(suspended)['aria-label'])).toInclude('멈')
    expect(messageOf(suspended)).not.toInclude('일시정지')
    expect(String(propsOf(suspended)['aria-label'])).not.toInclude('일시정지')

    expect(propsOf(reconnecting)).toHaveProperty('data-state', 'reconnecting')
    expect(messageOf(reconnecting)).toBe('스위치 다시 찾는 중')
    expect(messageOf(reconnecting)).not.toInclude('일시정지')
    expect(propsOf(permission)).toHaveProperty('data-state', 'permission')
    expect(messageOf(permission)).toInclude('권한')
    expect(messageOf(permission)).not.toInclude('일시정지')
    expect(propsOf(action)).toHaveProperty('data-state', 'action-required')
    expect(messageOf(action)).toInclude('블루투스')
    expect(messageOf(action)).not.toInclude('일시정지')
  })

  it('keeps scan controls enabled for reconnect, permission, and suspension', async () => {
    const [{ StatusLine }, { SwitchButton }] = await Promise.all([
      import('@/components/StatusLine'),
      import('@/components/SwitchButton'),
    ])
    const scan = propsOf(
      SwitchButton({ cursor: 'scanning', label: '>', name: '다음으로' }),
    )

    for (const snapshot of TRANSPORT_SNAPSHOTS) {
      const connection = normalizeArduinoPayload(snapshot)
      const status = propsOf(StatusLine({ ...SCANNING, connection }))
      expect(status).toHaveProperty(
        'data-state',
        connectionSentinel(connection),
      )
      expect(status).toHaveProperty('data-mark', connectionMark(connection))
      expect(scan).not.toHaveProperty('disabled')
      expect(scan).not.toHaveProperty('aria-disabled')
    }
  })
})

describe('Arduino connection lifecycle page boundary', () => {
  beforeEach(() => {
    listeners.clear()
    invokeCalls.length = 0
    focus.mockClear()
    invokeResult = Promise.resolve(SNAPSHOT)
    transportSnapshotResult = Promise.resolve(EMPTY_TRANSPORT)
    profileResult = Promise.resolve({ onboarded: true })
    overlayResult = Promise.resolve()
    tauriEventMockState.unlistenFails = false
    globalThis.setTimeout = ((callback: TimerHandler) => {
      if (typeof callback === 'function') callback()
      return 0 as unknown as ReturnType<typeof setTimeout>
    }) as unknown as typeof setTimeout
    globalThis.setInterval = ((callback: TimerHandler) => {
      if (typeof callback === 'function') intervalTicks = () => callback()
      return 0 as unknown as ReturnType<typeof setInterval>
    }) as unknown as typeof setInterval
    HTMLElement.prototype.focus = focus
  })

  afterEach(() => {
    globalThis.setTimeout = originalSetTimeout
    globalThis.setInterval = originalSetInterval
    document.body.innerHTML = ''
  })

  it('renders lifecycle events without disabling or focusing scan controls', async () => {
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)

    expect(listeners.has(ARDUINO_EVENT)).toBe(true)

    await act(async () => {
      emit('scan://state', SNAPSHOT)
      emit('scan://error', { message: 'scan error' })
      emit('scan://interval', { reason: 'interval changed' })
      emit('scan://preset', { message: 'preset changed' })
      emit('window://cover', { covered: true, percent: 50 })
      emit(ARDUINO_EVENT, { state: 'connected', port: 'port-a' })
    })
    expect(
      view.container.querySelector('[data-state]')?.getAttribute('data-state'),
    ).toBe('connected')

    await act(async () => {
      emit(ARDUINO_EVENT, { state: 'reconnecting' })
    })

    await act(async () => {
      emit('scan://state', { ...SNAPSHOT, mode: 'confirm' })
    })
    await act(async () => {
      emit('scan://state', { ...SNAPSHOT, mode: 'paused' })
    })
    await act(async () => {
      emit('scan://state', { ...SNAPSHOT, mode: 'dwelling' })
    })

    const scan = scanControl(view.container)
    expect(
      view.container.querySelector('[data-state]')?.getAttribute('data-state'),
    ).toBe('reconnecting')
    expect(scan.getAttribute('aria-label')).toBe('다음으로')
    expect(scan.hasAttribute('disabled')).toBe(false)
    expect(scan.hasAttribute('aria-disabled')).toBe(false)
    await act(async () => {
      intervalTicks?.()
    })
    expect(focus).not.toHaveBeenCalled()
    await act(async () => {
      view.unmount()
    })

    invokeResult = Promise.reject(
      new Error('Tauri is unavailable in this test'),
    )
    transportSnapshotResult = Promise.reject(
      new Error('transport snapshot unavailable'),
    )
    tauriEventMockState.unlistenFails = true
    const rejected = render(<FloatingPage />)
    await act(async () => {
      rejected.unmount()
    })
  })

  it('keeps scan controls enabled when Android settings events fire', async () => {
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)
    await act(async () => {
      window.dispatchEvent(new Event('hanbeon://open-settings'))
    })
    await act(async () => {
      window.dispatchEvent(new Event('hanbeon://open-floating'))
    })
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()
    await act(async () => {
      view.unmount()
    })
  })

  it('closes settings when the Android profile cannot be loaded', async () => {
    profileResult = Promise.reject(new Error('profile unavailable'))
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)
    await act(async () => {
      window.dispatchEvent(new Event('hanbeon://open-settings'))
      await profileResult.catch(() => {})
    })
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()
    await act(async () => {
      view.unmount()
    })
  })

  it('starts the overlay on Android without focusing scan controls', async () => {
    const navigator = globalThis.navigator as Navigator & {
      userAgent: string
    }
    const previous = navigator.userAgent
    Object.defineProperty(navigator, 'userAgent', {
      configurable: true,
      value: 'Mozilla/5.0 (Linux; Android 14)',
    })
    overlayResult = Promise.reject(new Error('overlay unavailable'))
    profileResult = Promise.resolve({ onboarded: false })
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)
    await act(async () => {
      await profileResult
      await overlayResult.catch(() => {})
    })
    expect(invokeCalls.includes('start_overlay_service')).toBe(true)
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()
    await act(async () => {
      view.unmount()
    })
    Object.defineProperty(navigator, 'userAgent', {
      configurable: true,
      value: previous,
    })
  })

  it('keeps scan controls enabled when Android profile loading fails', async () => {
    const navigator = globalThis.navigator as Navigator & {
      userAgent: string
    }
    const previous = navigator.userAgent
    Object.defineProperty(navigator, 'userAgent', {
      configurable: true,
      value: 'Mozilla/5.0 (Linux; Android 14)',
    })
    profileResult = Promise.reject(new Error('profile unavailable'))
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)
    await act(async () => {
      await profileResult.catch(() => {})
    })
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()
    await act(async () => {
      view.unmount()
    })
    Object.defineProperty(navigator, 'userAgent', {
      configurable: true,
      value: previous,
    })
  })

  it('applies the sticky native snapshot when the listener mounts after state exists', async () => {
    transportSnapshotResult = Promise.resolve({
      revision: 21,
      code: 'ready',
      active: 'ble',
      usb: 'lost',
      ble: 'ready',
      held: false,
      suspended: false,
      paused: false,
    })
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)

    await act(async () => {
      await transportSnapshotResult
    })

    expect(invokeCalls.includes('transport_status_snapshot')).toBe(true)
    expect(statusState(view.container)).toBe('ble-active')
    expect(statusMark(view.container)).toBe('ble')
    expect(
      view.container.querySelector('[data-state]')?.getAttribute('aria-label'),
    ).toInclude('블루투스')
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()

    await act(async () => {
      emit(ARDUINO_EVENT, {
        revision: 22,
        code: 'reconnecting',
        active: null,
        usb: 'starting',
        ble: 'lost',
        held: false,
        suspended: false,
        paused: false,
      })
    })
    expect(statusState(view.container)).toBe('reconnecting')
    expect(statusMark(view.container)).toBe('reconnect')
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()
    await act(async () => {
      view.unmount()
    })
  })

  it('ignores a stale native snapshot after a newer revision is shown', async () => {
    transportSnapshotResult = Promise.resolve({
      revision: 30,
      code: 'ready',
      active: 'usb',
      usb: 'ready',
      ble: 'ready',
      held: false,
      suspended: false,
      paused: false,
    })
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)
    await act(async () => {
      await transportSnapshotResult
    })
    expect(statusState(view.container)).toBe('usb-active')

    await act(async () => {
      emit(ARDUINO_EVENT, {
        revision: 31,
        code: 'suspended',
        active: null,
        usb: 'lost',
        ble: 'lost',
        held: false,
        suspended: true,
        paused: false,
      })
    })
    expect(statusState(view.container)).toBe('suspended')

    await act(async () => {
      emit(ARDUINO_EVENT, {
        revision: 30,
        code: 'ready',
        active: 'usb',
        usb: 'ready',
        ble: 'ready',
        held: false,
        suspended: false,
        paused: false,
      })
    })
    expect(statusState(view.container)).toBe('suspended')
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    await act(async () => {
      view.unmount()
    })
  })

  it('does not let a late snapshot overwrite a live desktop event', async () => {
    let resolveSnapshot: (value: unknown) => void = () => {}
    transportSnapshotResult = new Promise((resolve) => {
      resolveSnapshot = resolve
    })
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)
    await act(async () => {
      emit(ARDUINO_EVENT, { state: 'connected', port: 'port-a' })
    })
    expect(statusState(view.container)).toBe('connected')
    await act(async () => {
      resolveSnapshot({
        revision: 1,
        code: 'ready',
        active: 'ble',
        usb: 'stopped',
        ble: 'ready',
        held: false,
        suspended: false,
        paused: false,
      })
      await transportSnapshotResult
    })
    expect(statusState(view.container)).toBe('connected')
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    await act(async () => {
      view.unmount()
    })
  })

  it('keeps user pause distinct from transport suspension and leaves scan controls enabled', async () => {
    const { default: FloatingPage } = await import('@/app/page')
    const view = render(<FloatingPage />)
    await act(async () => {
      await invokeResult
      await transportSnapshotResult
    })
    await act(async () => {
      emit('scan://state', { ...SNAPSHOT, mode: 'paused' })
      emit(ARDUINO_EVENT, {
        revision: 40,
        code: 'paused',
        active: 'usb',
        usb: 'ready',
        ble: 'stopped',
        held: false,
        suspended: false,
        paused: true,
      })
    })
    expect(statusState(view.container)).toBe('usb-active')
    expect(statusMark(view.container)).toBe('usb')
    expect(statusCopy(view.container)).toInclude('일시정지')
    expect(statusCopy(view.container)).not.toInclude('스위치가 없어')
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()

    await act(async () => {
      emit('scan://state', { ...SNAPSHOT, mode: 'paused' })
      emit(ARDUINO_EVENT, {
        revision: 41,
        code: 'suspended',
        active: null,
        usb: 'lost',
        ble: 'lost',
        held: false,
        suspended: true,
        paused: false,
      })
    })
    expect(statusState(view.container)).toBe('suspended')
    expect(statusMark(view.container)).toBe('suspend')
    expect(statusCopy(view.container)).toInclude('스위치가 없어')
    expect(statusCopy(view.container)).not.toInclude('일시정지')
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()

    await act(async () => {
      emit(ARDUINO_EVENT, {
        revision: 42,
        code: 'reconnecting',
        active: null,
        usb: 'starting',
        ble: 'lost',
        held: false,
        suspended: false,
        paused: false,
      })
    })
    expect(statusState(view.container)).toBe('reconnecting')
    expect(statusMark(view.container)).toBe('reconnect')
    expect(statusCopy(view.container)).toInclude('다시')
    expect(statusCopy(view.container)).not.toInclude('일시정지')
    expect(scanControl(view.container).hasAttribute('disabled')).toBe(false)
    expect(focus).not.toHaveBeenCalled()
    await act(async () => {
      view.unmount()
    })
  })
})

describe('BLE caregiver setup stays independent of USB status', () => {
  it('keeps USB connected while BLE permission is denied', () => {
    expect(connectionSentinel({ state: 'connected', port: 'port-b' })).toBe(
      'connected',
    )
    expect(connectionCopy({ state: 'connected', port: 'port-b' })).toBeNull()
    expect(
      connectionSentinel(normalizeArduinoPayload({ code: 'permission' })),
    ).toBe('permission')
  })
})
