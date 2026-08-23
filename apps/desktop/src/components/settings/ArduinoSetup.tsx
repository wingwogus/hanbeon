'use client'

import { Box, Flex, Text, VStack } from '@devup-ui/react'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

import {
  type ArduinoCandidate,
  beginFirmwareInstall,
  canBeginInstall,
  cancelFirmwareInstall,
  candidateLabel,
  FIRMWARE_COPY,
  FIRMWARE_EVENT,
  firmwareOwnsPort,
  type FirmwareState,
  firmwareStatusText,
  INITIAL_FIRMWARE_STATE,
  listArduinoCandidates,
  probeArduinoFirmware,
} from '@/lib/firmware'

type Confirmation = Extract<FirmwareState, { state: 'confirmationRequired' }>

function ActionButton({
  children,
  disabled = false,
  onClick,
  primary = false,
}: {
  children: string
  disabled?: boolean
  onClick: () => void
  primary?: boolean
}) {
  return (
    <Box
      as="button"
      bg={primary ? '$primary' : '$scanIdleBg'}
      borderColor={primary ? '$primary' : '$borderBold'}
      borderRadius="12px"
      borderStyle="solid"
      borderWidth="2px"
      color={primary ? '$base' : '$text'}
      cursor={disabled ? 'default' : 'pointer'}
      disabled={disabled}
      onClick={() => {
        if (disabled) return
        onClick()
      }}
      px="28px"
      py="18px"
      type="button"
      typography="bodyL"
    >
      {children}
    </Box>
  )
}

function ChoiceButton({
  label,
  name,
  onSelect,
  selected,
}: {
  label: string
  name?: string
  onSelect: () => void
  selected: boolean
}) {
  return (
    <Box
      aria-label={name ?? label}
      aria-pressed={selected}
      as="button"
      bg={selected ? '$primaryBgBold' : '$scanIdleBg'}
      borderColor={selected ? '$primary' : '$borderBold'}
      borderRadius="12px"
      borderStyle="solid"
      borderWidth={selected ? '3px' : '2px'}
      color="$text"
      cursor="pointer"
      onClick={onSelect}
      px="20px"
      py="14px"
      textAlign="left"
      type="button"
      typography="bodyL"
    >
      {selected ? `● ${label}` : `○ ${label}`}
    </Box>
  )
}

function Card({
  children,
  description,
  title,
}: {
  children: React.ReactNode
  description?: string
  title: string
}) {
  return (
    <VStack
      bg="$containerBackground"
      borderColor="$border"
      borderRadius="16px"
      borderStyle="solid"
      borderWidth="1px"
      gap="12px"
      p="24px"
    >
      <Text as="h2" color="$title" typography="h2">
        {title}
      </Text>
      {description && (
        <Text color="$caption" typography="body">
          {description}
        </Text>
      )}
      {children}
    </VStack>
  )
}

function selectedCandidate(
  candidates: ArduinoCandidate[],
  selectedId: string | null,
) {
  if (selectedId) {
    return candidates.find((candidate) => candidate.deviceId === selectedId)
  }
  if (candidates.length === 1) return candidates[0]
  return undefined
}

export function ArduinoSetup({
  initialState = INITIAL_FIRMWARE_STATE,
  onComplete,
  onDefer,
  onLater,
}: {
  initialState?: FirmwareState
  onComplete: () => void
  onDefer?: () => void
  onLater?: () => void
}) {
  const defer = onDefer ?? onLater ?? (() => {})
  const [started, setStarted] = useState(initialState.state !== 'idle')
  const [firmware, setFirmware] = useState<FirmwareState>(initialState)
  const [selectedId, setSelectedId] = useState<string | null>(
    initialState.state === 'boardFound' && initialState.candidates.length === 1
      ? initialState.candidates[0].deviceId
      : null,
  )
  const [overwriteAcknowledged, setOverwriteAcknowledged] = useState(false)

  useEffect(() => {
    const unlisten = listen<FirmwareState>(FIRMWARE_EVENT, (event) => {
      setFirmware(event.payload)
    })
    return () => {
      unlisten.then((stop) => stop()).catch(() => {})
    }
  }, [])

  useEffect(() => {
    if (firmware.state === 'complete') onComplete()
  }, [firmware, onComplete])

  useEffect(() => {
    if (firmware.state !== 'confirmationRequired') {
      setOverwriteAcknowledged(false)
    }
    if (firmware.state === 'boardFound' && firmware.candidates.length === 1) {
      setSelectedId(firmware.candidates[0].deviceId)
    }
    if (firmware.state === 'searching' || firmware.state === 'idle') {
      setSelectedId(null)
    }
  }, [firmware])

  const owning = firmwareOwnsPort(firmware)
  const candidates = firmware.state === 'boardFound' ? firmware.candidates : []
  const confirmation: Confirmation | null =
    firmware.state === 'confirmationRequired' ? firmware : null
  const differentFirmware = confirmation?.reason === 'differentFirmware'
  const installEnabled = confirmation
    ? canBeginInstall(confirmation, overwriteAcknowledged) && !owning
    : false
  const chosen = selectedCandidate(candidates, selectedId)
  const status = firmwareStatusText(firmware)

  const startSearch = () => {
    setStarted(true)
    setSelectedId(null)
    setOverwriteAcknowledged(false)
    setFirmware({ state: 'searching' })
    void listArduinoCandidates()
      .then((candidates) => setFirmware({ state: 'boardFound', candidates }))
      .catch(() => {})
  }

  const probeSelected = () => {
    if (!chosen) return
    void probeArduinoFirmware(chosen.deviceId)
      .then(setFirmware)
      .catch((error: unknown) => {
        setFirmware({
          state: 'error',
          code: 'portUnavailable',
          retryable: true,
          detail: error instanceof Error ? error.message : String(error),
        })
      })
  }

  const install = () => {
    if (!confirmation || !installEnabled) return
    void beginFirmwareInstall(confirmation.deviceId).catch((error: unknown) => {
      setFirmware({
        state: 'error',
        code: 'uploadFailed',
        retryable: true,
        detail: error instanceof Error ? error.message : String(error),
      })
    })
  }

  const later = () => {
    if (started) void cancelFirmwareInstall().catch(() => {})
    defer()
  }

  const retry = () => {
    setSelectedId(null)
    setOverwriteAcknowledged(false)
    setFirmware({ state: 'searching' })
    void listArduinoCandidates().catch(() => {})
  }

  if (!started) {
    return (
      <Card
        description={FIRMWARE_COPY.unoOnly}
        title={FIRMWARE_COPY.startTitle}
      >
        <VStack gap="8px">
          <Text color="$text" typography="bodyL">
            {FIRMWARE_COPY.suppliesHeading}
          </Text>
          <Text color="$text" typography="bodyL">
            - {FIRMWARE_COPY.supplyUno}
          </Text>
          <Text color="$text" typography="bodyL">
            - {FIRMWARE_COPY.supplyUsb}
          </Text>
          <Text color="$text" typography="bodyL">
            - {FIRMWARE_COPY.supplyButton}
          </Text>
        </VStack>
        <Flex gap="12px">
          <ActionButton onClick={startSearch} primary>
            {FIRMWARE_COPY.startAction}
          </ActionButton>
          <ActionButton onClick={later}>{FIRMWARE_COPY.later}</ActionButton>
        </Flex>
      </Card>
    )
  }

  return (
    <Card
      description={
        firmware.state === 'boardFound' && candidates.length > 1
          ? FIRMWARE_COPY.chooseBoard
          : firmware.state === 'searching'
            ? FIRMWARE_COPY.connect
            : firmware.state === 'confirmationRequired'
              ? differentFirmware
                ? FIRMWARE_COPY.overwriteStrong
                : FIRMWARE_COPY.confirmNeed
              : owning
                ? FIRMWARE_COPY.installing
                : status
      }
      title={
        firmware.state === 'boardFound' && candidates.length > 1
          ? FIRMWARE_COPY.chooseBoard
          : firmware.state === 'searching'
            ? FIRMWARE_COPY.connect
            : status || FIRMWARE_COPY.connect
      }
    >
      <VStack
        aria-busy={owning || undefined}
        aria-live="polite"
        as="output"
        data-state={firmware.state}
        gap="12px"
      >
        {status && (
          <Text
            color={
              firmware.state === 'error'
                ? '$error'
                : firmware.state === 'alreadyInstalled' ||
                    firmware.state === 'complete'
                  ? '$success'
                  : differentFirmware
                    ? '$warning'
                    : '$text'
            }
            typography="bodyL"
          >
            {status}
          </Text>
        )}

        {firmware.state === 'boardFound' && chosen && (
          <Text color="$caption" typography="body">
            {candidateLabel(chosen)}
          </Text>
        )}

        {firmware.state === 'boardFound' && candidates.length > 1 && (
          <Flex flexWrap="wrap" gap="10px">
            {candidates.map((candidate, index) => (
              <ChoiceButton
                key={candidate.deviceId}
                label={candidateLabel(candidate)}
                name={`보드 ${index + 1}: ${candidateLabel(candidate)}`}
                onSelect={() => setSelectedId(candidate.deviceId)}
                selected={selectedId === candidate.deviceId}
              />
            ))}
          </Flex>
        )}

        {confirmation && (
          <VStack gap="12px">
            <Text color="$text" typography="bodyL">
              {FIRMWARE_COPY.confirmNeed}
            </Text>
            <Text
              color={differentFirmware ? '$warning' : '$text'}
              typography="bodyL"
            >
              {FIRMWARE_COPY.confirmReplace}
            </Text>
            {differentFirmware && (
              <Text color="$warning" typography="bodyL">
                {FIRMWARE_COPY.overwriteStrong}
              </Text>
            )}
            {confirmation.displayName && (
              <Text color="$caption" typography="body">
                {confirmation.displayName}
              </Text>
            )}
            {differentFirmware && (
              <ChoiceButton
                label={FIRMWARE_COPY.acknowledgeOverwrite}
                onSelect={() =>
                  setOverwriteAcknowledged((previous) => !previous)
                }
                selected={overwriteAcknowledged}
              />
            )}
          </VStack>
        )}

        {owning && (
          <Text color="$warning" typography="bodyL">
            {FIRMWARE_COPY.installing}
          </Text>
        )}
      </VStack>

      <Flex gap="12px">
        {firmware.state === 'alreadyInstalled' && (
          <ActionButton onClick={onComplete} primary>
            {FIRMWARE_COPY.continue}
          </ActionButton>
        )}
        {firmware.state === 'boardFound' && (
          <ActionButton disabled={!chosen} onClick={probeSelected} primary>
            {FIRMWARE_COPY.continue}
          </ActionButton>
        )}
        {confirmation && (
          <ActionButton disabled={!installEnabled} onClick={install} primary>
            {FIRMWARE_COPY.install}
          </ActionButton>
        )}
        {firmware.state === 'error' && firmware.retryable && (
          <ActionButton onClick={retry} primary>
            {FIRMWARE_COPY.retry}
          </ActionButton>
        )}
        <ActionButton disabled={owning} onClick={later}>
          {FIRMWARE_COPY.later}
        </ActionButton>
      </Flex>
    </Card>
  )
}
