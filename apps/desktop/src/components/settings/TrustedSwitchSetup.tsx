'use client'

import { Box, Flex, Text, VStack } from '@devup-ui/react'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

import { Section } from '@/components/settings/Section'
import {
  BLE_SETUP_EVENT,
  bleSetupCopy,
  bleSetupSentinel,
  type BleSetupSnapshot,
  getBleSetup,
  INITIAL_BLE_SETUP,
  isSafeBleLabel,
  requestBlePermission,
  revokeBleSwitch,
  sanitizeBleSetup,
  scanBleSwitches,
  selectBleSwitch,
} from '@/lib/ble-setup'

/**
 * 보호자가 믿을 블루투스 스위치를 하나 고르는 설정.
 *
 * 권한 요청은 이 화면의 버튼으로만 한다. 거절 뒤에 자동으로 다시 뜨지 않는다.
 * 식별자·GATT 오류는 화면에 올리지 않고, USB 스위치는 이 선택과 무관하게 유지한다.
 */
export function TrustedSwitchSetup() {
  const [snapshot, setSnapshot] = useState<BleSetupSnapshot>(INITIAL_BLE_SETUP)

  useEffect(() => {
    let fromEvent = false
    const apply = (next: BleSetupSnapshot, source: 'event' | 'invoke') => {
      if (source === 'event') fromEvent = true
      else if (fromEvent) return
      setSnapshot(sanitizeBleSetup(next))
    }
    void (async () => {
      try {
        apply(await getBleSetup(), 'invoke')
      } catch {
        // 데스크톱이나 브라우저 미리보기에는 네이티브 명령이 없다.
      }
    })()

    const unlisten = listen<BleSetupSnapshot>(BLE_SETUP_EVENT, (event) =>
      apply(event.payload, 'event'),
    )
    const onWindow = (event: Event) => {
      const detail = (event as CustomEvent<BleSetupSnapshot>).detail
      if (detail) apply(detail, 'event')
    }
    window.addEventListener(BLE_SETUP_EVENT, onWindow)

    return () => {
      unlisten.then((stop) => stop()).catch(() => {})
      window.removeEventListener(BLE_SETUP_EVENT, onWindow)
    }
  }, [])

  const applyResult = (next: BleSetupSnapshot) =>
    setSnapshot(sanitizeBleSetup(next))
  const candidates = snapshot.candidates.filter((candidate) =>
    isSafeBleLabel(candidate.label),
  )
  const selectedLabel =
    snapshot.label && isSafeBleLabel(snapshot.label) ? snapshot.label : null
  const notice = bleSetupCopy(snapshot)
  const denied = snapshot.code === 'permission-denied'
  const selected = snapshot.code === 'selected'
  const searching = snapshot.scanning || snapshot.code === 'scanning'

  return (
    <Section
      description="보호자가 믿을 블루투스 스위치를 하나만 고릅니다. USB 스위치는 권한·선택과 관계없이 그대로 쓸 수 있습니다."
      title="블루투스 스위치"
    >
      <VStack
        data-ble-ready={snapshot.readyToConnect ? 'true' : 'false'}
        data-ble-state={bleSetupSentinel(snapshot)}
        gap="12px"
      >
        <VStack
          bg={denied ? '$undoBg' : selected ? '$primaryBg' : '$scanIdleBg'}
          borderColor={
            denied ? '$undoText' : selected ? '$primary' : '$borderBold'
          }
          borderRadius="12px"
          borderStyle="solid"
          borderWidth={selected || denied ? '3px' : '2px'}
          gap="6px"
          p="20px"
        >
          <Text
            color={denied ? '$undoText' : selected ? '$title' : '$text'}
            typography="bodyL"
          >
            {selected ? `● ${selectedLabel ?? '한번 블루투스 스위치'}` : notice}
          </Text>
          {selected && (
            <Text color="$caption" typography="body">
              {notice}
            </Text>
          )}
        </VStack>

        {denied && snapshot.canRequestPermission && (
          <Box
            aria-label="블루투스 권한 허용"
            as="button"
            bg="$primary"
            borderRadius="12px"
            color="$base"
            cursor="pointer"
            onClick={() => {
              void (async () => {
                try {
                  applyResult(await requestBlePermission())
                } catch {
                  // 데스크톱이나 브라우저 미리보기에는 네이티브 명령이 없다.
                }
              })()
            }}
            px="24px"
            py="16px"
            typography="bodyL"
            w="fit-content"
          >
            블루투스 권한 허용
          </Box>
        )}

        {(snapshot.code === 'no-selection' || snapshot.code === 'scanning') && (
          <Box
            aria-label="근처 스위치 찾기"
            as="button"
            bg={searching ? '$primaryBg' : '$scanIdleBg'}
            borderColor={searching ? '$primary' : '$borderBold'}
            borderRadius="12px"
            borderStyle="solid"
            borderWidth={searching ? '3px' : '2px'}
            color="$text"
            cursor="pointer"
            onClick={() => {
              if (searching) return
              void (async () => {
                try {
                  applyResult(await scanBleSwitches())
                } catch {
                  // 데스크톱이나 브라우저 미리보기에는 네이티브 명령이 없다.
                }
              })()
            }}
            px="24px"
            py="16px"
            typography="bodyL"
            w="fit-content"
          >
            {searching ? '● 찾는 중' : '근처 스위치 찾기'}
          </Box>
        )}

        {candidates.length > 0 && (
          <Flex flexWrap="wrap" gap="10px">
            {candidates.map((candidate) => (
              <Box
                key={candidate.token}
                aria-label={`${candidate.label} 선택`}
                as="button"
                bg="$scanIdleBg"
                borderColor="$borderBold"
                borderRadius="12px"
                borderStyle="solid"
                borderWidth="2px"
                color="$text"
                cursor="pointer"
                onClick={() => {
                  void (async () => {
                    try {
                      applyResult(await selectBleSwitch(candidate.token))
                    } catch {
                      // 데스크톱이나 브라우저 미리보기에는 네이티브 명령이 없다.
                    }
                  })()
                }}
                px="20px"
                py="14px"
                typography="bodyL"
              >
                ○ {candidate.label}
              </Box>
            ))}
          </Flex>
        )}

        {selected && (
          <Box
            aria-label="블루투스 스위치 지우기"
            as="button"
            bg="$scanIdleBg"
            borderColor="$borderBold"
            borderRadius="12px"
            borderStyle="solid"
            borderWidth="2px"
            color="$text"
            cursor="pointer"
            onClick={() => {
              void (async () => {
                try {
                  applyResult(await revokeBleSwitch())
                } catch {
                  // 데스크톱이나 브라우저 미리보기에는 네이티브 명령이 없다.
                }
              })()
            }}
            px="24px"
            py="16px"
            typography="bodyL"
            w="fit-content"
          >
            블루투스 스위치 지우기
          </Box>
        )}
      </VStack>
    </Section>
  )
}
