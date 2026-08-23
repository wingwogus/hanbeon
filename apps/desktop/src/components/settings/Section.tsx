'use client'

import { Text, VStack } from '@devup-ui/react'

/**
 * 설정 항목 한 덩어리.
 *
 * 계층 메뉴를 만들지 않고 모두 펼쳐 두므로, 각 덩어리가 스스로 무엇인지
 * 설명해야 한다(PRD F8, 초기 설정 10분 이내).
 */
export function Section({
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
      <Text color="$title" typography="h2">
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
