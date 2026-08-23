'use client'

import { Flex, Text, VStack } from '@devup-ui/react'

interface LabelFormFieldProps {
  label: string
  caption?: string
  children?: React.ReactNode
  optional?: boolean
  error?: string
}
export function Label({
  label,
  caption,
  children,
  optional = false,
  error,
}: LabelFormFieldProps) {
  return (
    <VStack alignSelf="flex-end" gap="10px">
      <VStack gap={1}>
        <Flex
          alignItems="flex-start"
          flexWrap="wrap"
          justifyContent="space-between"
        >
          <Flex alignItems="center" gap={1}>
            <Text color="$title" typography="bodyS600" whiteSpace="nowrap">
              {label}
            </Text>
            {optional && (
              <Text color="$caption" typography="bodyS" whiteSpace="nowrap">
                (선택 사항)
              </Text>
            )}
          </Flex>
          <Text color="$error" textAlign="right" typography="bodyS">
            {error}
          </Text>
        </Flex>
        {caption && (
          <Text color="$caption" typography="caption">
            {caption}
          </Text>
        )}
      </VStack>
      {children}
    </VStack>
  )
}
