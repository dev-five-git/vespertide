'use client'

import { Button } from '@devup-ui/react'
import {
  cloneElement,
  ComponentProps,
  createContext,
  isValidElement,
  useContext,
  useState,
} from 'react'

const PopoverContext = createContext<{
  open: boolean
  setOpen: (open: boolean) => void
} | null>(null)

export function usePopover() {
  const context = useContext(PopoverContext)
  if (!context) {
    throw new Error('usePopover must be used within a PopoverProvider')
  }
  return context
}

export function Popover({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <PopoverContext.Provider value={{ open, setOpen }}>
      {children}
    </PopoverContext.Provider>
  )
}

export function PopoverTrigger({
  asChild,
  children,
  ...props
}: ComponentProps<typeof Button<'button'>> & {
  asChild?: boolean
  children: React.ReactNode
}) {
  const { open, setOpen } = usePopover()

  if (asChild) {
    const child = isValidElement(children) ? children : null
    if (!child) return null
    return cloneElement(child, {
      onClick: () => setOpen(!open),
      ...props,
    })
  }

  return (
    <Button
      bg="transparent"
      border="none"
      onClick={() => setOpen(!open)}
      p="0"
      styleOrder={1}
      {...props}
    >
      {children}
    </Button>
  )
}
