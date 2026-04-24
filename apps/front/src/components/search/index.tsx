import { Input } from '@devup-ui/components'
import { Text } from '@devup-ui/react'
import { Box, Center, css, VStack } from '@devup-ui/react'
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

export function SearchResult(
  props: Omit<ComponentProps<typeof VStack<'div'>>, 'children'>,
) {
  return (
    <VStack
      bg="$containerBackground"
      borderRadius="16px"
      left="50%"
      maxH="600px"
      overflow="hidden"
      p="$spacingSpacing24"
      pos="absolute"
      styleOrder={1}
      top="88px"
      transform="translateX(-50%)"
      zIndex="110"
      {...props}
    >
      <Center py="40px">
        <Text color="$caption" flex="1" textAlign="center" typography="caption">
          No search results found.
        </Text>
      </Center>
    </VStack>
  )
}
