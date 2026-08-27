import { mock } from 'bun:test'

import type { Profile, SaveResult } from '@/lib/profile'

/**
 * Bun replaces the whole module, so two suites each mocking a slice of
 * `@/lib/profile` deleted the exports the other one needed: whichever loaded
 * second won, and the loser saw "Export named 'closeSettings' not found".
 *
 * One double owns the module and suites override individual behaviours.
 */
export const profileMock = {
  getProfile: () => Promise.resolve({ onboarded: true }) as Promise<unknown>,
  saveProfile: (profile: unknown) =>
    Promise.resolve({ profile, warning: null }) as Promise<unknown>,
  closeSettings: () => Promise.resolve(),
  getLogDirectory: () => Promise.resolve('/tmp/hanbeon-logs'),
}

mock.module('@/lib/profile', () => ({
  getProfile: () => profileMock.getProfile() as Promise<Profile>,
  saveProfile: (next: Profile) =>
    profileMock.saveProfile(next) as Promise<SaveResult>,
  closeSettings: () => profileMock.closeSettings(),
  getLogDirectory: () => profileMock.getLogDirectory(),
}))
