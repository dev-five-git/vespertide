import { Flex } from '@devup-ui/react'
import { ComponentProps } from 'react'

export function JoinIconButton(props: ComponentProps<typeof Flex<'div'>>) {
  return (
    <Flex
      _active={{
        bg: '#0A6F3699',
      }}
      _hover={{
        bg: '#0A6F3666',
      }}
      alignItems="center"
      bg="#0A6F3640"
      borderRadius="100px"
      p="16px"
      styleOrder={1}
      {...props}
    />
  )
}
