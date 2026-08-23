import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test'
import { render } from 'bun-test-env-dom'
import { act, type ReactElement } from 'react'

import {
  ARDUINO_EVENT,
  type ArduinoConnection,
  connectionCopy,
  connectionSentinel,
  INITIAL_CONNECTION,
} from '@/lib/arduino'

type Listener = (event: { payload: unknown }) => void

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

const LIFECYCLE: ReadonlyArray<ArduinoConnection> = [
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

const listeners = new Map<string, Listener>()
const focus = mock(() => {})
const originalSetTimeout = globalThis.setTimeout
const originalSetInterval = globalThis.setInterval
let intervalTicks: (() => void) | undefined
let invokeResult: Promise<unknown> = Promise.resolve(SNAPSHOT)
let unlistenFails = false

mock.module('@tauri-apps/api/event', () => ({
  TauriEvent: {},
  emit: () => Promise.resolve(),
  emitTo: () => Promise.resolve(),
  listen: (event: string, listener: Listener) => {
    if (unlistenFails) return Promise.reject(new Error('unlisten unavailable'))
    listeners.set(event, listener)
    return Promise.resolve(() => listeners.delete(event))
  },
  once: () => Promise.resolve(() => {}),
}))

mock.module('@tauri-apps/api/core', () => ({
  SERIALIZE_TO_IPC_FN: '__TAURI_TO_IPC_KEY__',
  invoke: () => invokeResult,
}))

mock.module('@/components/DragHandle', () => ({
  DragHandle: () => <div aria-hidden="true" />,
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
})

describe('Arduino connection lifecycle page boundary', () => {
  beforeEach(() => {
    listeners.clear()
    focus.mockClear()
    invokeResult = Promise.resolve(SNAPSHOT)
    unlistenFails = false
    globalThis.setTimeout = ((callback: TimerHandler) => {
      if (typeof callback === 'function') callback()
      return 0 as unknown as ReturnType<typeof setTimeout>
    }) as typeof setTimeout
    globalThis.setInterval = ((callback: TimerHandler) => {
      if (typeof callback === 'function') intervalTicks = callback
      return 0 as unknown as ReturnType<typeof setInterval>
    }) as typeof setInterval
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
    unlistenFails = true
    const rejected = render(<FloatingPage />)
    await act(async () => {
      rejected.unmount()
    })
  })
})
