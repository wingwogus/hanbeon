import { Flex, Text } from '@devup-ui/react'
import Link from 'next/link'

import { LogoutButton } from '../Buttons/LogoutButton'
import { ThemeButton } from '../Buttons/ThemeButton'
import { HeaderMobileContainer } from './HeaderMobileContainer'

export function Header() {
  return (
    <Flex
      alignItems="center"
      bg="$containerBackground"
      borderBottom="1px solid $border"
      h={['54px', null, '60px']}
      justifyContent="space-between"
      left={0}
      pl={4}
      pos="fixed"
      pr={[0, null, 4]}
      top={0}
      w="100%"
      zIndex={100}
    >
      <Link href="/dashboard">
        <Text color="$title" typography="h6">
          한번 실증
        </Text>
      </Link>
      <Flex
        alignItems="center"
        display={['none', null, 'flex']}
        flexWrap="wrap"
        gap={2.5}
        h="100%"
        justifyContent="flex-end"
      >
        <ThemeButton />
        <LogoutButton />
      </Flex>

      <HeaderMobileContainer />
    </Flex>
  )
}
