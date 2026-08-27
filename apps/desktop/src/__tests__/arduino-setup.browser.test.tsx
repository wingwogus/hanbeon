import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test'
import { fireEvent, render } from 'bun-test-env-dom'
import { act, createElement, type ReactNode } from 'react'

import {
  type ArduinoCandidate,
  FIRMWARE_COMMANDS,
  FIRMWARE_COPY,
  FIRMWARE_EVENT,
  type FirmwareState,
} from '../lib/firmware'
import { profileMock } from './profile.mock'

type Listener = (event: { payload: unknown }) => void

const UNO: ArduinoCandidate = {
  deviceId: 'candidate-1',
  displayName: 'Arduino Uno',
  port: '/dev/cu.usbmodem1401',
  vid: 0x2341,
  pid: 0x0043,
}

const UNO_TWO: ArduinoCandidate = {
  deviceId: 'candidate-2',
  displayName: 'Arduino Uno',
  port: '/dev/cu.usbmodem1402',
  vid: 0x2341,
  pid: 0x0043,
}

const PROFILE = {
  intervalMs: 1800,
  minIntervalMs: 600,
  maxIntervalMs: 4000,
  adaptive: true,
  manualLock: false,
  longPressMs: 800,
  switchKey: 'F13',
  sound: true,
  undoMapping: 'undo' as const,
  theme: 'light' as const,
  windowPosition: null,
  dimWhenCovered: true,
  dimPercent: 40,
  appButtons: true,
  logging: false,
  onboarded: false,
}

const listeners = new Map<string, Listener>()
const commands: { name: string; args: unknown }[] = []
let invokeImpl: (
  name: string,
  args?: Record<string, unknown>,
) => Promise<unknown> = async () => undefined

let unlistenFails = false

mock.module('@tauri-apps/api/event', () => ({
  TauriEvent: {},
  emit: () => Promise.resolve(),
  emitTo: () => Promise.resolve(),
  listen: (event: string, listener: Listener) => {
    listeners.set(event, listener)
    if (unlistenFails) {
      return Promise.resolve(() => {
        throw new Error('unlisten unavailable')
      })
    }
    return Promise.resolve(() => listeners.delete(event))
  },
  once: () => Promise.resolve(() => {}),
}))

mock.module('@tauri-apps/api/core', () => ({
  SERIALIZE_TO_IPC_FN: '__TAURI_TO_IPC_KEY__',
  invoke: (name: string, args?: Record<string, unknown>) => {
    commands.push({ name, args })
    return invokeImpl(name, args)
  },
}))

mock.module('@devup-ui/react', () => {
  const passthrough = (fallback: string) => {
    function DevupElement({
      as,
      children,
      ...props
    }: Record<string, unknown> & { as?: string; children?: ReactNode }) {
      const Tag = (as as string | undefined) ?? fallback
      const dom: Record<string, unknown> = {}
      for (const [key, value] of Object.entries(props)) {
        if (
          key.startsWith('aria-') ||
          key.startsWith('data-') ||
          key === 'role' ||
          key === 'disabled' ||
          key === 'type' ||
          key === 'onClick' ||
          key === 'id' ||
          key === 'title'
        ) {
          dom[key] = value
        }
      }
      return createElement(Tag, dom, children)
    }
    return DevupElement
  }
  return {
    Box: passthrough('div'),
    Flex: passthrough('div'),
    Text: passthrough('p'),
    VStack: passthrough('div'),
    Center: passthrough('div'),
  }
})

mock.module('@/components/settings/Section', () => {
  return {
    Section: ({
      children,
      description,
      title,
    }: {
      children: ReactNode
      description?: string
      title: string
    }) =>
      createElement(
        'section',
        null,
        createElement('h2', null, title),
        description ? createElement('p', null, description) : null,
        children,
      ),
  }
})

mock.module('@/components/settings/Range', () => {
  return {
    Range: ({
      label,
      onChange,
      valueText,
    }: {
      label: string
      onChange: (next: number) => void
      valueText: string
    }) =>
      createElement(
        'button',
        { onClick: () => onChange(2000), type: 'button' },
        `${label} ${valueText}`,
      ),
  }
})

mock.module('@/components/settings/SwitchTester', () => ({
  SwitchTester: () => null,
}))

mock.module('@/lib/format', () => ({
  formatSeconds: (ms: number) => `${(ms / 1000).toFixed(1)}초`,
}))

profileMock.saveProfile = (profile: unknown) => {
  commands.push({ name: 'save_profile', args: { next: profile } })
  return Promise.reject(new Error('save unavailable'))
}

function emit(event: string, payload: unknown) {
  const listener = listeners.get(event)
  if (!listener) throw new Error(`No listener registered for ${event}`)
  listener({ payload })
}

function textOf(container: Element) {
  return container.textContent ?? ''
}

function buttonNamed(container: Element, label: string) {
  return [...container.querySelectorAll('button')].find(
    (button) => button.textContent === label,
  )
}

async function setupView(initialState?: FirmwareState) {
  const { ArduinoSetup } = await import('../components/settings/ArduinoSetup')
  const onComplete = mock(() => {})
  const onDefer = mock(() => {})
  const view = render(
    <ArduinoSetup
      initialState={initialState}
      onComplete={onComplete}
      onDefer={onDefer}
    />,
  )
  await act(async () => {})
  return { view, onComplete, onDefer }
}

describe('Arduino firmware setup screens', () => {
  beforeEach(() => {
    listeners.clear()
    commands.length = 0
    invokeImpl = async () => undefined
    unlistenFails = false
    document.body.innerHTML = ''
  })

  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('shows the start guide and does not issue an install command yet', async () => {
    const { view } = await setupView()

    expect(textOf(view.container)).toInclude(FIRMWARE_COPY.startTitle)
    expect(textOf(view.container)).toInclude(FIRMWARE_COPY.supplyUno)
    expect(textOf(view.container)).toInclude(FIRMWARE_COPY.supplyUsb)
    expect(textOf(view.container)).toInclude(FIRMWARE_COPY.supplyButton)
    expect(buttonNamed(view.container, FIRMWARE_COPY.startAction)).toBeTruthy()
    expect(
      commands.some(
        (command) => command.name === FIRMWARE_COMMANDS.beginInstall,
      ),
    ).toBe(false)
  })

  it('subscribes to firmware events before listing boards', async () => {
    const order: string[] = []
    invokeImpl = async (name) => {
      order.push(`invoke:${name}`)
      return []
    }
    const originalSet = listeners.set.bind(listeners)
    listeners.set = ((event: string, listener: Listener) => {
      order.push(`listen:${event}`)
      return originalSet(event, listener)
    }) as typeof listeners.set

    const { ArduinoSetup } = await import('../components/settings/ArduinoSetup')
    const view = render(
      <ArduinoSetup onComplete={() => {}} onDefer={() => {}} />,
    )
    await act(async () => {})
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, FIRMWARE_COPY.startAction)!)
    })

    expect(order[0]).toBe(`listen:${FIRMWARE_EVENT}`)
    expect(order).toContain(`invoke:${FIRMWARE_COMMANDS.listCandidates}`)
    expect(order.indexOf(`listen:${FIRMWARE_EVENT}`)).toBeLessThan(
      order.indexOf(`invoke:${FIRMWARE_COMMANDS.listCandidates}`),
    )
  })

  it('asks the user to connect a board while searching', async () => {
    const { view } = await setupView()
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, FIRMWARE_COPY.startAction)!)
      emit(FIRMWARE_EVENT, { state: 'searching' } satisfies FirmwareState)
    })

    expect(textOf(view.container)).toInclude(
      'Arduino Uno를 USB로 연결해 주세요',
    )
    expect(
      view.container.querySelector('[data-state]')?.getAttribute('data-state'),
    ).toBe('searching')
    expect(view.container.querySelector('output')?.textContent).toInclude(
      'Arduino Uno를 USB로 연결해 주세요',
    )
  })

  it('shows a found Uno without using the port path as the label', async () => {
    const { view } = await setupView({
      state: 'boardFound',
      candidates: [UNO],
    })

    expect(textOf(view.container)).toInclude('Arduino Uno를 찾았습니다')
    expect(textOf(view.container)).toInclude('Arduino Uno')
    expect(textOf(view.container)).toInclude('2341:0043')
    expect(textOf(view.container)).not.toInclude('/dev/cu.usbmodem1401')
    expect(
      commands.some(
        (command) => command.name === FIRMWARE_COMMANDS.beginInstall,
      ),
    ).toBe(false)
  })

  it('requires an explicit choice before probing when several Unos are present', async () => {
    const { view } = await setupView({
      state: 'boardFound',
      candidates: [UNO, UNO_TWO],
    })

    expect(textOf(view.container)).toInclude(
      '연결할 Arduino Uno를 선택해 주세요',
    )
    expect(buttonNamed(view.container, '다음')?.hasAttribute('disabled')).toBe(
      true,
    )

    const choices = [...view.container.querySelectorAll('[aria-pressed]')]
    expect(choices).toHaveLength(2)
    await act(async () => {
      fireEvent.click(choices[1]!)
    })
    expect(buttonNamed(view.container, '다음')?.hasAttribute('disabled')).toBe(
      false,
    )

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '다음')!)
    })
    expect(commands.at(-1)).toEqual({
      name: FIRMWARE_COMMANDS.probe,
      args: { deviceId: 'candidate-2', device_id: 'candidate-2' },
    })
    expect(
      commands.some(
        (command) => command.name === FIRMWARE_COMMANDS.beginInstall,
      ),
    ).toBe(false)
  })

  it('surfaces a probe failure instead of staying on the found-board screen', async () => {
    invokeImpl = async (name) => {
      if (name === FIRMWARE_COMMANDS.probe) {
        throw new Error('invalid args device_id')
      }
      return undefined
    }
    const { view } = await setupView({
      state: 'boardFound',
      candidates: [UNO],
    })

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '다음')!)
    })

    expect(commands.at(-1)).toEqual({
      name: FIRMWARE_COMMANDS.probe,
      args: { deviceId: 'candidate-1', device_id: 'candidate-1' },
    })
    expect(textOf(view.container)).toInclude('포트를 사용할 수 없습니다')
    expect(textOf(view.container)).toInclude('invalid args device_id')
    expect(
      view.container.querySelector('[data-state]')?.getAttribute('data-state'),
    ).toBe('error')
  })

  it('skips upload when the dedicated firmware is already installed', async () => {
    const { view, onComplete } = await setupView({
      state: 'alreadyInstalled',
      deviceId: 'candidate-1',
    })

    expect(textOf(view.container)).toInclude(
      '전용 펌웨어가 이미 설치되어 있습니다',
    )
    expect(textOf(view.container)).not.toInclude('펌웨어 설치')
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '다음')!)
    })
    expect(onComplete).toHaveBeenCalledTimes(1)
    expect(
      commands.some(
        (command) => command.name === FIRMWARE_COMMANDS.beginInstall,
      ),
    ).toBe(false)
  })

  it('asks for install consent when the board does not answer', async () => {
    const { view } = await setupView({
      state: 'confirmationRequired',
      deviceId: 'candidate-1',
      reason: 'noResponse',
      confirmationToken: 'token-1',
    })

    expect(textOf(view.container)).toInclude(
      '버튼을 사용하려면 전용 펌웨어를 설치해야 합니다.',
    )
    expect(textOf(view.container)).toInclude(
      '설치하면 현재 Arduino에 들어 있는 기존 스케치는 교체됩니다.',
    )
    expect(textOf(view.container)).not.toInclude(FIRMWARE_COPY.overwriteStrong)

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '펌웨어 설치')!)
    })
    expect(commands.at(-1)).toEqual({
      name: FIRMWARE_COMMANDS.beginInstall,
      args: { deviceId: 'candidate-1', device_id: 'candidate-1' },
    })
  })

  it('keeps install disabled until the stronger overwrite warning is acknowledged', async () => {
    const { view } = await setupView({
      state: 'confirmationRequired',
      deviceId: 'candidate-1',
      reason: 'differentFirmware',
      confirmationToken: 'token-1',
    })

    expect(textOf(view.container)).toInclude(FIRMWARE_COPY.overwriteStrong)
    expect(textOf(view.container)).toInclude(
      '설치하면 현재 Arduino에 들어 있는 기존 스케치는 교체됩니다.',
    )
    expect(
      buttonNamed(view.container, '펌웨어 설치')?.hasAttribute('disabled'),
    ).toBe(true)

    await act(async () => {
      fireEvent.click(
        buttonNamed(
          view.container,
          `● ${FIRMWARE_COPY.acknowledgeOverwrite}`,
        ) ??
          buttonNamed(
            view.container,
            `○ ${FIRMWARE_COPY.acknowledgeOverwrite}`,
          )!,
      )
    })
    expect(
      buttonNamed(view.container, '펌웨어 설치')?.hasAttribute('disabled'),
    ).toBe(false)

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '펌웨어 설치')!)
    })
    expect(commands.at(-1)?.name).toBe(FIRMWARE_COMMANDS.beginInstall)
  })

  it('shows preparing, uploading and verifying without a second install action', async () => {
    for (const state of [
      { state: 'preparing', deviceId: 'candidate-1' },
      { state: 'uploading', deviceId: 'candidate-1', progress: 40 },
      { state: 'verifying', deviceId: 'candidate-1' },
    ] satisfies FirmwareState[]) {
      const { view } = await setupView(state)
      expect(textOf(view.container)).toInclude(
        state.state === 'preparing'
          ? 'Arduino 준비 중'
          : state.state === 'uploading'
            ? '펌웨어 전송 중'
            : '설치 확인 중',
      )
      expect(textOf(view.container)).toInclude('설치가 진행 중')
      expect(textOf(view.container)).not.toInclude('펌웨어 설치')
      expect(view.container.querySelector('[aria-busy="true"]')).not.toBeNull()
      view.unmount()
    }
  })

  it('lets the user continue later without saving onboarding completion', async () => {
    const { view, onComplete, onDefer } = await setupView({
      state: 'confirmationRequired',
      deviceId: 'candidate-1',
      reason: 'noResponse',
      confirmationToken: 'token-1',
    })

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '나중에 하기')!)
    })
    expect(onDefer).toHaveBeenCalledTimes(1)
    expect(onComplete).not.toHaveBeenCalled()
    expect(commands.some((command) => command.name === 'save_profile')).toBe(
      false,
    )
  })

  it('retries only when the installer says the error is retryable', async () => {
    const { view } = await setupView({
      state: 'error',
      code: 'uploadFailed',
      retryable: true,
    })
    expect(textOf(view.container)).toInclude('펌웨어 전송에 실패했습니다')
    expect(textOf(view.container)).not.toInclude('usbmodem')

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '다시 시도')!)
    })
    expect(commands.at(-1)?.name).toBe(FIRMWARE_COMMANDS.listCandidates)

    const blocked = await setupView({
      state: 'error',
      code: 'verifyFailed',
      retryable: false,
    })
    expect(textOf(blocked.view.container)).toInclude(
      '설치는 끝났지만 펌웨어 확인에 실패했습니다',
    )
    expect(buttonNamed(blocked.view.container, '다시 시도')).toBeUndefined()
    expect(buttonNamed(blocked.view.container, '나중에 하기')).toBeTruthy()
  })

  it('cleans up the firmware listener on unmount', async () => {
    const { view } = await setupView()
    expect(listeners.has(FIRMWARE_EVENT)).toBe(true)
    await act(async () => {
      view.unmount()
    })
    expect(listeners.has(FIRMWARE_EVENT)).toBe(false)
  })

  it('completes after a successful install event and accepts onLater', async () => {
    const { ArduinoSetup } = await import('../components/settings/ArduinoSetup')
    const onComplete = mock(() => {})
    const onLater = mock(() => {})
    const view = render(
      <ArduinoSetup onComplete={onComplete} onLater={onLater} />,
    )
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '나중에 하기')!)
    })
    expect(onLater).toHaveBeenCalledTimes(1)
    await act(async () => {
      emit(FIRMWARE_EVENT, {
        state: 'complete',
        deviceId: 'candidate-1',
      } satisfies FirmwareState)
    })
    expect(onComplete).toHaveBeenCalledTimes(1)
  })

  it('survives rejected invoke and shows the named overwrite board', async () => {
    invokeImpl = async () => {
      throw new Error('command unavailable')
    }
    unlistenFails = true
    const { ArduinoSetup } = await import('../components/settings/ArduinoSetup')
    const unnamed = render(<ArduinoSetup onComplete={() => {}} />)
    await act(async () => {
      fireEvent.click(buttonNamed(unnamed.container, '시작하기')!)
    })
    await act(async () => {
      unnamed.unmount()
    })
    unlistenFails = false
    const { view } = await setupView({
      state: 'confirmationRequired',
      deviceId: 'candidate-1',
      reason: 'differentFirmware',
      confirmationToken: 'token-1',
      displayName: 'Arduino Uno',
    })
    expect(textOf(view.container)).toInclude('Arduino Uno')
    await act(async () => {
      emit(FIRMWARE_EVENT, { state: 'searching' } satisfies FirmwareState)
    })
    expect(
      view.container.querySelector('[data-state]')?.getAttribute('data-state'),
    ).toBe('searching')
  })
})

describe('Onboarding keeps firmware setup before switch testing', () => {
  beforeEach(() => {
    listeners.clear()
    commands.length = 0
    invokeImpl = async () => undefined
    unlistenFails = false
    document.body.innerHTML = ''
  })

  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('starts on the Arduino guide and can continue to switch testing later', async () => {
    const { Onboarding } = await import('../components/settings/Onboarding')
    const onDone = mock(() => {})
    const view = render(<Onboarding initial={PROFILE} onDone={onDone} />)
    await act(async () => {})

    expect(textOf(view.container)).toInclude('Arduino 연결')
    expect(textOf(view.container)).toInclude(FIRMWARE_COPY.startTitle)
    expect(textOf(view.container)).not.toInclude(
      '스위치가 연결됐는지 확인합니다',
    )

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '나중에 하기')!)
    })

    expect(textOf(view.container)).toInclude('스위치가 연결됐는지 확인합니다')
    expect(onDone).not.toHaveBeenCalled()
    expect(commands.some((command) => command.name === 'save_profile')).toBe(
      false,
    )
  })

  it('keeps later Arduino onboarding steps available after firmware setup', async () => {
    const { Onboarding } = await import('../components/settings/Onboarding')
    const onDone = mock(() => {})
    const view = render(<Onboarding initial={PROFILE} onDone={onDone} />)

    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '나중에 하기')!)
    })
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '다음')!)
    })
    expect(textOf(view.container)).toInclude('속도를 맞춥니다')
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '주사 속도 1.8초')!)
    })
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '다음')!)
    })
    expect(textOf(view.container)).toInclude('저장합니다')
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '이전')!)
    })
    expect(textOf(view.container)).toInclude('속도를 맞춥니다')
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '다음')!)
    })
    await act(async () => {
      fireEvent.click(buttonNamed(view.container, '저장하고 시작')!)
    })
    expect(onDone).toHaveBeenCalled()
  })
})
