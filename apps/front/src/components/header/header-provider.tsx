'use client'

import { createContext, useContext, useEffect, useMemo, useState } from 'react'

import { useSheet } from '../sheet'

const HeaderContext = createContext<{
  menuOpen: boolean
  setMenuOpen: (menuOpen: boolean) => void
  transparent: boolean
  sentinels: Set<HTMLElement>
  isSentinelVisible: boolean
} | null>(null)

export function useHeader() {
  const context = useContext(HeaderContext)
  if (!context) {
    throw new Error('useHeader must be used within a HeaderProvider')
  }
  return context
}

export function HeaderProvider({ children }: { children: React.ReactNode }) {
  const { isOpen } = useSheet()
  const [menuOpen, setMenuOpen] = useState(false)
  const transparent = !isOpen
  const sentinels = useMemo<Set<HTMLElement>>(() => new Set(), [])
  const [isSentinelVisible, setIsSentinelVisible] = useState(false)

  const io = useMemo(() => {
    if (typeof window === 'undefined') return null
    return new IntersectionObserver(
      (entries) => {
        entries.some((entry) => entry.isIntersecting)
          ? setIsSentinelVisible(true)
          : setIsSentinelVisible(false)
      },
      {
        rootMargin: `-68px 0px -${window.innerHeight - 68}px 0px`,
      },
    )
  }, [])

  useEffect(() => {
    if (!io) return
    sentinels.forEach((element) => {
      io.observe(element)
    })
    return () => {
      sentinels.forEach((element) => {
        io.unobserve(element)
      })
    }
  }, [io, sentinels])

  return (
    <HeaderContext.Provider
      value={{
        menuOpen,
        setMenuOpen,
        transparent,
        sentinels,
        isSentinelVisible,
      }}
    >
      {children}
    </HeaderContext.Provider>
  )
}
