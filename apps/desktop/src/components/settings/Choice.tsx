'use client'

import { Box, Flex } from '@devup-ui/react'

/**
 * 두세 개 중 하나를 고르는 항목.
 *
 * 라디오 버튼보다 누르기 쉬운 큰 버튼으로 만들고, 선택 여부를 색이 아니라
 * 기호(● / ○)로도 표시한다.
 */
export function Choice<T extends string>({
  onChange,
  options,
  value,
}: {
  onChange: (next: T) => void
  options: { label: string; value: T }[]
  value: T
}) {
  return (
    <Flex flexWrap="wrap" gap="10px">
      {options.map((option) => {
        const selected = option.value === value
        return (
          <Box
            key={option.value}
            aria-pressed={selected}
            as="button"
            bg={selected ? '$primaryBgBold' : '$scanIdleBg'}
            borderColor={selected ? '$primary' : '$borderBold'}
            borderRadius="12px"
            borderStyle="solid"
            borderWidth={selected ? '3px' : '2px'}
            color="$text"
            cursor="pointer"
            onClick={() => onChange(option.value)}
            px="20px"
            py="14px"
            typography="bodyL"
          >
            {selected ? `● ${option.label}` : `○ ${option.label}`}
          </Box>
        )
      })}
    </Flex>
  )
}
