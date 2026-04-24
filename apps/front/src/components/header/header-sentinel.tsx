'use client'

import { Box } from '@devup-ui/react'
import { ComponentPropsWithoutRef } from 'react'

import { useHeader } from './header-provider'

export function HeaderSentinel(
  props: ComponentPropsWithoutRef<typeof Box<'div'>>,
) {
  const { sentinels } = useHeader()
  return (
    <Box
      ref={(node) => {
        if (!node) return
        sentinels.add(node)
        return () => {
          sentinels.delete(node)
        }
      }}
      {...props}
    />
  )
}
