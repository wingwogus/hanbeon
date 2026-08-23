'use client'

import { Box, Center } from '@devup-ui/react'
import { useRouter } from 'next/navigation'

export function BackButton() {
  const router = useRouter()
  return (
    <Center
      _active={{
        bg: '$primaryBgBold',
      }}
      _hover={{
        bg: '$primaryBg',
      }}
      bg="$containerBackground"
      border="1px solid $borderBold"
      borderRadius={2}
      boxSize={9}
      cursor="pointer"
      onClick={() => router.back()}
    >
      <Box
        bg="$text"
        boxSize="24px"
        maskImage="url(/icons/arrow.svg)"
        maskPosition="center"
        maskRepeat="no-repeat"
        maskSize="24px"
        rotate="270deg"
      />
    </Center>
  )
}
