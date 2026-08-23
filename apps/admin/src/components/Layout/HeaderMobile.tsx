'use client'
import { Center } from '@devup-ui/react'

export function HeaderMobile({
  open,
  setOpen,
}: {
  open: boolean
  setOpen: () => void
}) {
  return (
    <Center
      cursor="pointer"
      display={['flex', null, 'none']}
      onClick={setOpen}
      p={4}
    >
      <Center
        bg="$title"
        boxSize={6}
        maskImage={open ? 'url(/icons/close.svg)' : 'url(/icons/hamburger.svg)'}
        maskPosition="center"
        maskRepeat="no-repeat"
        maskSize="contain"
      />
    </Center>
  )
}
