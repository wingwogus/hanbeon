import { invoke } from '@tauri-apps/api/core'

/**
 * Caregiver BLE setup contract.
 *
 * Native Android owns permission prompts, scan, and the persisted identity.
 * This module is only the machine-consumed snapshot and Korean copy. Raw
 * MAC addresses and GATT errors stay out of user-facing text.
 */
export const BLE_SETUP_EVENT = 'ble://setup'

export type BleSetupCode =
  | 'permission-denied'
  | 'bluetooth-off'
  | 'no-selection'
  | 'selected'
  | 'unavailable'
  | 'scanning'

export interface BleCandidate {
  token: string
  label: string
}

export interface BleSetupSnapshot {
  code: BleSetupCode
  label: string | null
  usbUsable: boolean
  readyToConnect: boolean
  canRequestPermission: boolean
  scanning: boolean
  candidates: BleCandidate[]
}

export const INITIAL_BLE_SETUP: BleSetupSnapshot = {
  code: 'no-selection',
  label: null,
  // Neutral browser/desktop fallback; Android replaces this with the arbiter snapshot.
  usbUsable: true,
  readyToConnect: false,
  canRequestPermission: false,
  scanning: false,
  candidates: [],
}

const MAC = /([0-9a-f]{2}:){5}[0-9a-f]{2}/i
const GATT = /gatt|status\s*=?\s*\d+/i

export const bleSetupSentinel = (snapshot: BleSetupSnapshot) => snapshot.code

export const isSafeBleLabel = (value: string | null | undefined) => {
  if (!value) return false
  return !MAC.test(value) && !GATT.test(value)
}

const redactCandidates = (candidates: BleCandidate[]) =>
  candidates.filter((candidate) => isSafeBleLabel(candidate.label))

export const bleSetupCopy = (snapshot: BleSetupSnapshot): string => {
  const usb = snapshot.usbUsable
    ? ' USB 스위치는 그대로 쓸 수 있습니다.'
    : ' USB 스위치 연결 상태를 확인해 주세요.'
  switch (snapshot.code) {
    case 'permission-denied':
      return `블루투스 권한이 필요합니다.${usb}`
    case 'bluetooth-off':
      return `블루투스를 켜 주세요.${usb}`
    case 'no-selection':
      return `선택된 블루투스 스위치가 없습니다.${usb}`
    case 'selected':
      return `${snapshot.label && isSafeBleLabel(snapshot.label) ? snapshot.label : '한번 블루투스 스위치'}를 신뢰합니다. USB가 없으면 이 스위치로 이어집니다.`
    case 'unavailable':
      return `이 기기에서는 블루투스 스위치를 쓸 수 없습니다.${usb}`
    case 'scanning':
      return `근처의 한번 스위치를 찾는 중입니다.${usb}`
  }
}

export const getBleSetup = () => invoke<BleSetupSnapshot>('ble_setup_snapshot')

export const requestBlePermission = () =>
  invoke<BleSetupSnapshot>('ble_setup_request_permission')

export const scanBleSwitches = () => invoke<BleSetupSnapshot>('ble_setup_scan')

export const selectBleSwitch = (token: string) =>
  invoke<BleSetupSnapshot>('ble_setup_select', { token })

export const revokeBleSwitch = () =>
  invoke<BleSetupSnapshot>('ble_setup_revoke')

export const sanitizeBleSetup = (
  snapshot: BleSetupSnapshot,
): BleSetupSnapshot => {
  const label = isSafeBleLabel(snapshot.label) ? snapshot.label : null
  const candidates = redactCandidates(snapshot.candidates)
  if (snapshot.code === 'selected' && !label) {
    return {
      ...snapshot,
      code: 'no-selection',
      label: null,
      readyToConnect: false,
      candidates,
      usbUsable: snapshot.usbUsable,
    }
  }
  return {
    ...snapshot,
    label,
    candidates,
    usbUsable: snapshot.usbUsable,
    readyToConnect: snapshot.code === 'selected' && label !== null,
  }
}
