'use client'

import { Box, Flex, Grid, Image, Text, VStack } from '@devup-ui/react'
import { usePathname } from 'next/navigation'
import { useReducer } from 'react'

import { MenuItem } from './MenuItem'

export function MenuItemWithChildren({
  id,
  label,
  childrens,
}: {
  id: string
  label: string
  childrens?: { id: string; label: string; link?: string }[]
}) {
  const pathname = usePathname()
  const [open, setOpen] = useReducer((state) => !state, pathname.includes(id))
  return (
    <VStack gap={[0, null, 1]}>
      <Flex
        _active={{ bg: [null, null, '$primaryBgBold'] }}
        _hover={{ bg: [null, null, '$primaryBg'] }}
        alignItems="center"
        borderRadius={[0, null, 2]}
        cursor="pointer"
        gap={[0, null, 2.5]}
        justifyContent="space-between"
        onClick={setOpen}
        px={[6, null, 2.5]}
        py={[4, null, 3]}
        transition=".25s"
      >
        <Text color="$title" flex="1" fontWeight="500" typography="bodyS">
          {label}
        </Text>
        <Image
          alt="arrow"
          rotate={open ? undefined : '180deg'}
          src="/icons/arrow.svg"
          transition=".2s"
        />
      </Flex>
      <Grid
        gridTemplateRows={open ? '1fr' : '0fr'}
        overflow="hidden"
        transition="grid-template-rows 0.3s ease-in-out"
      >
        <Flex
          bg={['$primaryBg', null, 'none']}
          borderBottom={open && ['1px solid $border', null, 'none']}
          borderTop={open && ['1px solid $border', null, 'none']}
          minH={0}
          pl={[0, null, 3]}
        >
          <Box
            bg="$border"
            display={['none', null, 'block']}
            h="100%"
            w="1px"
          />
          <VStack
            flex={1}
            gap={[0, null, 1]}
            pl={[0, null, 2.5]}
            py={[0, null, 1]}
          >
            {childrens?.map(({ id, label, link }) => (
              <MenuItem key={id} id={id} label={label} link={link} />
            ))}
          </VStack>
        </Flex>
      </Grid>
    </VStack>
  )
}
