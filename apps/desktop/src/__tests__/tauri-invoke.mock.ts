import { mock } from 'bun:test'

type InvokeHandler = (
  command: string,
  args?: unknown,
) => Promise<unknown> | undefined

/**
 * Bun's module mocks are process-global: the last suite to call
 * `mock.module('@tauri-apps/api/core', ...)` wins for every suite in the run.
 * Registering competing doubles made whichever file loaded second answer the
 * other file's commands with null, which silently changed what the component
 * rendered — the onboarding suite passed alone and failed in the full run.
 *
 * So there is one double, and suites contribute handlers to it. A handler that
 * returns undefined declines the command and the next handler gets a turn.
 */
const handlers: InvokeHandler[] = []

export const invokeCalls: { command: string; args?: unknown }[] = []

/** Adds a handler and returns a disposer, so suites can clean up in afterEach. */
export const registerInvokeHandler = (handler: InvokeHandler) => {
  handlers.unshift(handler)
  return () => {
    const at = handlers.indexOf(handler)
    if (at >= 0) handlers.splice(at, 1)
  }
}

mock.module('@tauri-apps/api/core', () => ({
  SERIALIZE_TO_IPC_FN: '__TAURI_TO_IPC_KEY__',
  invoke: (command: string, args?: unknown) => {
    invokeCalls.push({ command, args })
    for (const handler of handlers) {
      const result = handler(command, args)
      if (result !== undefined) return result
    }
    return Promise.resolve(null)
  },
}))
