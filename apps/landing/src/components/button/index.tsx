import { Box, Center, Flex, Text } from '@devup-ui/react'
import { ComponentProps } from 'react'

export function Button({
  children,
  ...props
}: ComponentProps<typeof Center<'button'>>) {
  return (
    <Center
      _active={{
        bg: '#172B20',
        color: '$vesperaPrimary',
      }}
      _hover={{
        bg: '#172B20',
      }}
      as="button"
      bg="#10131F"
      border="none"
      borderRadius="100px"
      color="$vesperaBg"
      cursor="pointer"
      px={['28px', null, null, null, '$spacingSpacing32']}
      py={['10px', null, null, null, '$spacingSpacing12']}
      styleOrder={1}
      transition="all .1s"
      {...props}
    >
      <Box bg="$secondary" borderRadius="50%" boxSize="10px" display="none" />
      <Flex alignItems="center">
        <Text color="#FDF6DE" typography="bodyLgEb">
          {children}
        </Text>
      </Flex>
    </Center>
  )
}
