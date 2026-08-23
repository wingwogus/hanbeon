'use client'
import { Box, Flex, Text, VStack } from '@devup-ui/react'
import { Fragment } from 'react'

import { MENU_ITEMS } from '../../menu_items'
import { MenuItem } from './MenuItem'
import { MenuItemWithChildren } from './MenuItemWithChildren'

export function SideBar() {
  return (
    <VStack
      bg="$containerBackground"
      borderRight="1px solid $border"
      display={['none', null, 'block']}
      gap={2.5}
      minH="100vh"
      p={2.5}
      pt={18}
      w={['100%', null, '260px']}
    >
      {MENU_ITEMS.map(({ label, items }, idx) => (
        <Fragment key={label}>
          <VStack gap={1}>
            <Flex px={1.5} py={1}>
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
                <MenuItem key={value.id} {...value} />
              ),
            )}
          </VStack>
          {idx < MENU_ITEMS.length - 1 && (
            <Box bg="$border" h="1px" my={3} w="100%" />
          )}
        </Fragment>
      ))}
    </VStack>
  )
}
