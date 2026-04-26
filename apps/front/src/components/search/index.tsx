import { Input } from '@devup-ui/components'
import { Box, css } from '@devup-ui/react'
import { ComponentProps } from 'react'

export function Search(props: ComponentProps<typeof Input>) {
  return (
    <Input
      classNames={{
        container: css({ flex: '1' }),
        input: css({ w: '100%', pl: '48px', py: '8px', pr: '16px' }),
      }}
      colors={{
        primary: 'var(--vespertidePrimary)',
        primaryFocus: 'var(--vespertidePrimary)',
        border: 'var(--border)',
      }}
      icon={
        <Box
          aspectRatio="1"
          bg="$border"
          boxSize="20px"
          maskImage="url(/icons/search.svg)"
          maskPos="center"
          maskRepeat="no-repeat"
          maskSize="contain"
        />
      }
      name="search"
      placeholder="Search documentation"
      typography="caption"
      {...props}
    />
  )
}
