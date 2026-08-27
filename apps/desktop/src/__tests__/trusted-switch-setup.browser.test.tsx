import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test'
import { render } from 'bun-test-env-dom'
import { act, type ReactElement } from 'react'

import {
  BLE_SETUP_EVENT,
  bleSetupCopy,
  bleSetupSentinel,
  type BleSetupSnapshot,
  INITIAL_BLE_SETUP,
  isSafeBleLabel,
} from '@/lib/ble-setup'
import { type Profile } from '@/lib/profile'

import { tauriEventListeners as listeners } from './tauri-event.mock'

const originalSetTimeout = globalThis.setTimeout

const PROFILE: Profile = {
  intervalMs: 1800,
  minIntervalMs: 800,
  maxIntervalMs: 4000,
  adaptive: true,
  manualLock: false,
  longPressMs: 600,
  switchKey: 'F13',
  sound: true,
  undoMapping: 'back',
  theme: 'light',
  windowPosition: null,
  dimWhenCovered: true,
  dimPercent: 40,
  appButtons: true,
  logging: true,
  onboarded: true,
}

const DENIED: BleSetupSnapshot = {
  code: 'permission-denied',
  label: null,
  usbUsable: true,
  readyToConnect: false,
  canRequestPermission: true,
  scanning: false,
  candidates: [],
}

const BLUETOOTH_OFF: BleSetupSnapshot = {
  code: 'bluetooth-off',
  label: null,
  usbUsable: true,
  readyToConnect: false,
  canRequestPermission: false,
  scanning: false,
  candidates: [],
}

const ABSENT: BleSetupSnapshot = {
  code: 'no-selection',
  label: null,
  usbUsable: true,
  readyToConnect: false,
  canRequestPermission: false,
  scanning: false,
  candidates: [
    { token: 'ble-1', label: 'HanBeon XIAO' },
    { token: 'ble-2', label: '00:11:22:33:44:55' },
    { token: 'ble-3', label: 'GATT error 133' },
  ],
}

const SELECTED: BleSetupSnapshot = {
  code: 'selected',
  label: 'HanBeon XIAO',
  usbUsable: true,
  readyToConnect: true,
  canRequestPermission: false,
  scanning: false,
  candidates: [],
}

const SCANNING: BleSetupSnapshot = {
  code: 'scanning',
  label: null,
  usbUsable: true,
  readyToConnect: false,
  canRequestPermission: false,
  scanning: true,
  candidates: [],
}

const STALE_SELECTED: BleSetupSnapshot = {
  code: 'selected',
  label: 'AA:BB:CC:DD:EE:FF',
  usbUsable: true,
  readyToConnect: true,
  canRequestPermission: false,
  scanning: false,
  candidates: [],
}

let snapshot: BleSetupSnapshot = { ...DENIED }
let invokeCalls: Array<{ command: string; args?: unknown }> = []
let persistFile: BleSetupSnapshot | null = null

mock.module('@tauri-apps/api/core', () => ({
  SERIALIZE_TO_IPC_FN: '__TAURI_TO_IPC_KEY__',
  invoke: (command: string, args?: unknown) => {
    invokeCalls.push({ command, args })
    if (command === 'get_profile') return Promise.resolve(PROFILE)
    if (command === 'save_profile') {
      return Promise.resolve({ profile: PROFILE, warning: null })
    }
    if (command === 'log_directory') return Promise.resolve('/tmp/hanbeon-logs')
    if (command === 'close_settings') return Promise.resolve()
    if (command === 'ble_setup_snapshot') {
      return new Promise((resolve) => {
        queueMicrotask(() => resolve(persistFile ?? snapshot))
      })
    }
    if (command === 'ble_setup_request_permission') {
      return Promise.resolve(persistFile ?? snapshot)
    }
    if (command === 'ble_setup_scan') {
      return Promise.resolve(persistFile ?? snapshot)
    }
    if (command === 'ble_setup_select') {
      persistFile = { ...SELECTED }
      snapshot = persistFile
      return Promise.resolve(persistFile)
    }
    if (command === 'ble_setup_revoke') {
      persistFile = { ...ABSENT, candidates: [] }
      snapshot = persistFile
      return Promise.resolve(persistFile)
    }
    return Promise.resolve(null)
  },
}))

function emit(event: string, payload: unknown) {
  const listener = listeners.get(event)
  if (!listener) throw new Error(`No listener registered for ${event}`)
  listener({ payload })
}

function propsOf(element: ReactElement) {
  return element.props as Record<string, unknown>
}

function textOf(node: Element | null) {
  return node?.textContent ?? ''
}

describe('BLE setup contract', () => {
  it.each([DENIED, BLUETOOTH_OFF, ABSENT, SELECTED, SCANNING] as const)(
    'exposes machine-consumed $code without identifiers or GATT errors',
    (state) => {
      expect(bleSetupSentinel(state)).toBe(state.code)
      expect(BLE_SETUP_EVENT).toBe('ble://setup')
      expect(JSON.stringify(bleSetupCopy(state))).not.toInclude(':')
      expect(JSON.stringify(bleSetupCopy(state))).not.toInclude('GATT')
      expect(JSON.stringify(bleSetupCopy(state))).not.toInclude('AA:BB')
      expect(state.usbUsable).toBe(true)
    },
  )

  it('does not treat a MAC or GATT string as a safe public label', () => {
    expect(isSafeBleLabel('HanBeon XIAO')).toBe(true)
    expect(isSafeBleLabel('AA:BB:CC:DD:EE:FF')).toBe(false)
    expect(isSafeBleLabel('status=133 GATT_ERROR')).toBe(false)
    expect(isSafeBleLabel(null)).toBe(false)
  })

  it('defaults to no trusted device and never auto-connects', () => {
    expect(bleSetupSentinel(INITIAL_BLE_SETUP)).toBe('no-selection')
    expect(INITIAL_BLE_SETUP.readyToConnect).toBe(false)
    expect(INITIAL_BLE_SETUP.usbUsable).toBe(true)
  })

  it('treats a stale selected MAC as absent and not connect-ready', async () => {
    const { sanitizeBleSetup } = await import('@/lib/ble-setup')
    const sanitized = sanitizeBleSetup(STALE_SELECTED)
    expect(sanitized.code).toBe('no-selection')
    expect(sanitized.readyToConnect).toBe(false)
    expect(sanitized.usbUsable).toBe(true)
    expect(JSON.stringify(sanitized)).not.toInclude('AA:BB')
  })
})

describe('Trusted switch setup UI', () => {
  beforeEach(() => {
    listeners.clear()
    invokeCalls = []
    persistFile = null
    snapshot = { ...DENIED }
    globalThis.setTimeout = ((callback: TimerHandler) => {
      if (typeof callback === 'function') callback()
      return 0 as unknown as ReturnType<typeof setTimeout>
    }) as unknown as typeof setTimeout
  })

  afterEach(() => {
    globalThis.setTimeout = originalSetTimeout
    document.body.innerHTML = ''
  })

  it('renders Android 31+ denial as actionable without disrupting USB', async () => {
    const { TrustedSwitchSetup } =
      await import('@/components/settings/TrustedSwitchSetup')
    const view = render(<TrustedSwitchSetup />)

    await act(async () => {
      emit(BLE_SETUP_EVENT, DENIED)
    })

    expect(
      view.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('permission-denied')
    expect(textOf(view.container)).toInclude('블루투스 권한')
    expect(textOf(view.container)).toInclude('USB')
    expect(textOf(view.container)).not.toInclude('GATT')
    expect(
      view.container.querySelector('[aria-label="블루투스 권한 허용"]'),
    ).not.toBeNull()
    expect(
      invokeCalls.some(
        (call) => call.command === 'ble_setup_request_permission',
      ),
    ).toBe(false)
    expect(view.container.querySelector('[data-ble-ready="true"]')).toBeNull()
  })

  it('does not auto-request permission after a denial snapshot arrives', async () => {
    const { TrustedSwitchSetup } =
      await import('@/components/settings/TrustedSwitchSetup')
    const view = render(<TrustedSwitchSetup />)

    await act(async () => {
      emit(BLE_SETUP_EVENT, DENIED)
    })

    expect(
      invokeCalls.filter(
        (call) => call.command === 'ble_setup_request_permission',
      ),
    ).toHaveLength(0)
    expect(
      view.container.querySelector('[aria-label="블루투스 권한 허용"]'),
    ).not.toBeNull()
  })

  it('renders scanning as not connect-ready and keeps USB usable', async () => {
    snapshot = { ...SCANNING }
    const { TrustedSwitchSetup } =
      await import('@/components/settings/TrustedSwitchSetup')
    const view = render(<TrustedSwitchSetup />)

    await act(async () => {
      emit(BLE_SETUP_EVENT, SCANNING)
    })

    expect(
      view.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('scanning')
    expect(
      view.container
        .querySelector('[data-ble-ready]')
        ?.getAttribute('data-ble-ready'),
    ).toBe('false')
    expect(textOf(view.container)).toInclude('찾는 중')
    expect(textOf(view.container)).toInclude('USB')
    expect(textOf(view.container)).not.toInclude('GATT')
  })

  it('renders Bluetooth off distinctly from missing selection', async () => {
    snapshot = { ...BLUETOOTH_OFF }
    const { TrustedSwitchSetup } =
      await import('@/components/settings/TrustedSwitchSetup')
    const view = render(<TrustedSwitchSetup />)

    await act(async () => {
      emit(BLE_SETUP_EVENT, BLUETOOTH_OFF)
    })

    expect(
      view.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('bluetooth-off')
    expect(textOf(view.container)).toInclude('블루투스를 켜')
    expect(textOf(view.container)).not.toInclude('선택된 스위치가 없습니다')
  })

  it('cannot connect without a trusted device and hides untrusted metadata', async () => {
    snapshot = { ...ABSENT }
    const { TrustedSwitchSetup } =
      await import('@/components/settings/TrustedSwitchSetup')
    const view = render(<TrustedSwitchSetup />)

    await act(async () => {
      emit(BLE_SETUP_EVENT, ABSENT)
    })

    expect(
      view.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('no-selection')
    expect(textOf(view.container)).toInclude('HanBeon XIAO')
    expect(textOf(view.container)).not.toInclude('00:11:22:33:44:55')
    expect(textOf(view.container)).not.toInclude('GATT error 133')
    expect(view.container.querySelector('[data-ble-ready="true"]')).toBeNull()
  })

  it('selects exactly one trusted XIAO then revokes it', async () => {
    snapshot = {
      ...ABSENT,
      candidates: [{ token: 'ble-1', label: 'HanBeon XIAO' }],
    }
    const { TrustedSwitchSetup } =
      await import('@/components/settings/TrustedSwitchSetup')
    const view = render(<TrustedSwitchSetup />)

    await act(async () => {
      emit(BLE_SETUP_EVENT, snapshot)
    })
    await act(async () => {
      view.container
        .querySelector('[aria-label="HanBeon XIAO 선택"]')
        ?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    expect(
      view.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('selected')
    expect(
      view.container
        .querySelector('[data-ble-ready]')
        ?.getAttribute('data-ble-ready'),
    ).toBe('true')
    expect(textOf(view.container)).toInclude('HanBeon XIAO')
    expect(
      invokeCalls.some(
        (call) =>
          call.command === 'ble_setup_select' &&
          JSON.stringify(call.args).includes('ble-1'),
      ),
    ).toBe(true)

    await act(async () => {
      view.container
        .querySelector('[aria-label="블루투스 스위치 지우기"]')
        ?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(
      view.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('no-selection')
    expect(
      view.container
        .querySelector('[data-ble-ready]')
        ?.getAttribute('data-ble-ready'),
    ).toBe('false')
  })

  it('survives remount from persisted selection without exposing identifiers', async () => {
    persistFile = { ...SELECTED }
    snapshot = persistFile
    const { TrustedSwitchSetup } =
      await import('@/components/settings/TrustedSwitchSetup')
    const first = render(<TrustedSwitchSetup />)
    await act(async () => {})
    expect(
      first.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('selected')
    await act(async () => {
      first.unmount()
    })

    const second = render(<TrustedSwitchSetup />)
    await act(async () => {})
    expect(
      second.container
        .querySelector('[data-ble-state]')
        ?.getAttribute('data-ble-state'),
    ).toBe('selected')
    expect(JSON.stringify(textOf(second.container))).not.toInclude(':')
    expect(textOf(second.container)).toInclude('HanBeon XIAO')
  })
})

describe('Settings and onboarding expose caregiver BLE setup', () => {
  beforeEach(() => {
    listeners.clear()
    invokeCalls = []
    persistFile = null
    snapshot = { ...ABSENT, candidates: [] }
    globalThis.setTimeout = ((callback: TimerHandler) => {
      if (typeof callback === 'function') callback()
      return 0 as unknown as ReturnType<typeof setTimeout>
    }) as unknown as typeof setTimeout
  })

  afterEach(() => {
    globalThis.setTimeout = originalSetTimeout
    document.body.innerHTML = ''
  })

  it('keeps USB usable from settings while BLE is denied', async () => {
    snapshot = { ...DENIED }
    const { SettingsForm } = await import('@/components/settings/SettingsForm')
    const view = render(<SettingsForm initial={PROFILE} />)

    await act(async () => {
      emit(BLE_SETUP_EVENT, DENIED)
    })

    expect(textOf(view.container)).toInclude('블루투스 스위치')
    expect(textOf(view.container)).toInclude('USB')
    expect(
      view.container.querySelector('[aria-label="블루투스 권한 허용"]'),
    ).not.toBeNull()
    expect(textOf(view.container)).toInclude('스위치 키')
  })

  it('lets onboarding select a trusted XIAO without blocking USB confirmation', async () => {
    snapshot = {
      ...ABSENT,
      candidates: [{ token: 'ble-1', label: 'HanBeon XIAO' }],
    }
    const { Onboarding } = await import('@/components/settings/Onboarding')
    const view = render(
      <Onboarding
        initial={{ ...PROFILE, onboarded: false }}
        onDone={() => {}}
      />,
    )

    await act(async () => {
      emit(BLE_SETUP_EVENT, snapshot)
    })

    expect(textOf(view.container)).toInclude('스위치를 눌러 보세요')
    expect(textOf(view.container)).toInclude('HanBeon XIAO')
    expect(textOf(view.container)).toInclude('USB')
  })
})

describe('Arduino USB lifecycle stays independent of BLE setup', () => {
  it('does not disable USB connection copy when BLE is denied', async () => {
    const { connectionCopy, connectionSentinel } = await import('@/lib/arduino')
    const { StatusLine } = await import('@/components/StatusLine')
    const usb = { state: 'connected', port: 'port-b' } as const

    expect(connectionSentinel(usb)).toBe('connected')
    expect(connectionCopy(usb)).toBeNull()
    expect(bleSetupCopy(DENIED)).toInclude('권한')
    expect(
      propsOf(
        StatusLine({
          connection: usb,
          intervalMs: 1800,
          mode: 'scanning',
          notice: null,
        }),
      ),
    ).toHaveProperty('data-state', 'connected')
  })
})
