'use client'

import { Box, Flex, Text, VStack } from '@devup-ui/react'

/** 슬라이더 한 줄. 현재 값을 항상 숫자로 함께 보여준다. */
export function Range({
  label,
  max,
  min,
  onChange,
  step = 100,
  value,
  valueText,
}: {
  label: string
  max: number
  min: number
  onChange: (next: number) => void
  step?: number
  value: number
  valueText: string
}) {
  return (
    <VStack gap="8px">
      <Flex alignItems="center" gap="12px" justifyContent="space-between">
        <Text color="$text" typography="bodyL">
          {label}
        </Text>
        <Text color="$primary" typography="bodyL">
          {valueText}
        </Text>
      </Flex>
      <Box
        aria-label={label}
        as="input"
        h="32px"
        max={max}
        min={min}
        onChange={(event: React.ChangeEvent<HTMLInputElement>) =>
          onChange(Number(event.target.value))
        }
        step={step}
        type="range"
        value={value}
        w="100%"
      />
    </VStack>
  )
}
