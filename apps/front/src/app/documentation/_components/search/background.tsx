'use client'

import { Dimmer } from '@/components/dimmer'
import { useSheetRouter } from '@/components/sheet/router'

export function Background() {
  const { route } = useSheetRouter()

  return <Dimmer dimmed={route === 'search'} />
}

export { Background as SearchBackground }
