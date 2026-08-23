'use client'

import { useReducer } from 'react'

import { MobileMenu } from '../SideMenu/MobileMenu'
import { HeaderMobile } from './HeaderMobile'

export function HeaderMobileContainer() {
  const [open, setOpen] = useReducer((state) => !state, false)
  return (
    <>
      <HeaderMobile open={open} setOpen={setOpen} />

      <MobileMenu isOpen={open} setIsOpen={setOpen} />
    </>
  )
}
