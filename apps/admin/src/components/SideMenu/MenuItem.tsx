'use client'

import { css, Flex, Text } from '@devup-ui/react'
import Link from 'next/link'
import { usePathname } from 'next/navigation'

export function MenuItem({
  id,
  label,
  link,
  onClick,
}: {
  id: string
  label: string
  link?: string
  onClick?: () => void
}) {
  const pathname = usePathname()
  const isSelected = pathname.includes(id)
  return (
    <Link
      className={css({ textDecoration: 'none' })}
      href={link ?? '/'}
      onClick={onClick}
    >
      <Flex
        _active={!isSelected && { bg: [null, null, '$primaryBgBold'] }}
        _hover={!isSelected && { bg: [null, null, '$primaryBg'] }}
        alignItems="center"
        bg={isSelected && '$primaryBgBold'}
        borderRadius={[0, null, '8px']}
        flex={1}
        gap={[0, null, 2.5]}
        justifyContent="space-between"
        px={[6, null, 2.5]}
        py={[4, null, 3]}
        transition=".25s"
      >
        <Text
          color={isSelected ? '$primary' : '$title'}
          flex="1"
          fontWeight={isSelected ? '700' : '500'}
          typography="bodyS"
        >
          {label}
        </Text>
      </Flex>
    </Link>
  )
}
