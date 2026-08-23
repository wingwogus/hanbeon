'use client'

import { Box } from '@devup-ui/react'

/**
 * 켜고 끄는 항목.
 *
 * 상태를 색이 아니라 글자로 말한다. '켜짐/꺼짐'을 색으로만 표시하면
 * 고대비 모드와 색약 사용자에게 아무 정보도 주지 못한다.
 */
export function Toggle({
  checked,
  offLabel,
  onChange,
  onLabel,
}: {
  checked: boolean
  offLabel: string
  onChange: (next: boolean) => void
  onLabel: string
}) {
  return (
    <Box
      aria-pressed={checked}
      as="button"
      bg={checked ? '$primary' : '$scanIdleBg'}
      borderColor={checked ? '$primary' : '$borderBold'}
      borderRadius="12px"
      borderStyle="solid"
      borderWidth={checked ? '3px' : '2px'}
      color={checked ? '$base' : '$text'}
      cursor="pointer"
      onClick={() => onChange(!checked)}
      px="24px"
      py="16px"
      typography="bodyL"
      w="fit-content"
    >
      {checked ? onLabel : offLabel}
    </Box>
  )
}
