import { Flex, Text, VStack } from '@devup-ui/react'

import { MENU_ITEMS } from '../../menu_items'
import { LogoutButton } from '../Buttons/LogoutButton'
import { ThemeButton } from '../Buttons/ThemeButton'
import { MenuItem } from './MenuItem'
import { MenuItemWithChildren } from './MenuItemWithChildren'

export function MobileMenu({
  isOpen,
  setIsOpen,
}: {
  isOpen: boolean
  setIsOpen: () => void
}) {
  return (
    <VStack
      bg="$containerBackground"
      h="calc(100dvh - 54px)"
      opacity={[1, null, 0]}
      overflow="auto"
      pos="fixed"
      right={isOpen ? 0 : '-100%'}
      top="54px"
      transition="right 0.3s ease-in-out"
      visibility={['visible', null, 'hidden']}
      w="100%"
      zIndex={100}
    >
      {MENU_ITEMS.map(({ label, items }) => (
        <VStack key={label} borderBottom="1px solid $border">
          <Flex px={3} py={3}>
            <Text color="$primary" flex="1" typography="caption700">
              {label}
            </Text>
          </Flex>
          {items.map((value) =>
            value.childrens ? (
              <MenuItemWithChildren
                key={value.id}
                childrens={value.childrens}
                id={value.id}
                label={value.label}
              />
            ) : (
              <MenuItem key={value.id} onClick={setIsOpen} {...value} />
            ),
          )}
        </VStack>
      ))}

      <VStack gap={3} pb={10} pt={4} px={4}>
        <Flex alignItems="center" gap={2} justifyContent="space-between">
          <ThemeButton />
          <LogoutButton />
        </Flex>
      </VStack>
    </VStack>
  )
}
