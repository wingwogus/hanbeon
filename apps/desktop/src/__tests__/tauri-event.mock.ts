import { mock } from 'bun:test'

type Listener = (event: { payload: unknown }) => void

/**
 * Bun's module mocks are process-global. All browser suites therefore use one
 * event double rather than registering competing module-local listener maps.
 */
export const tauriEventListeners = new Map<string, Listener>()
export const tauriEventMockState = { unlistenFails: false }

mock.module('@tauri-apps/api/event', () => ({
  TauriEvent: {},
  emit: () => Promise.resolve(),
  emitTo: () => Promise.resolve(),
  listen: (event: string, listener: Listener) => {
    if (tauriEventMockState.unlistenFails) {
      return Promise.resolve(() => {
        throw new Error('unlisten unavailable')
      })
    }
    tauriEventListeners.set(event, listener)
    return Promise.resolve(() => tauriEventListeners.delete(event))
  },
  once: () => Promise.resolve(() => {}),
}))
