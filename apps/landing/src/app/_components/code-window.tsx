import { Box, Flex, Text } from '@devup-ui/react'
import type { ComponentProps, ReactNode } from 'react'

import './code-theme'

export function CodeWindow({
  title,
  tabs,
  children,
  ...props
}: {
  title: string
  tabs?: ReactNode
  children: ReactNode
} & ComponentProps<typeof Box<'div'>>) {
  return (
    <Box
      bg="$cardBase"
      border="1px solid $border"
      borderRadius="$borderRadiusRadius12"
      boxShadow="0 30px 60px -30px rgba(0,0,0,.25)"
      overflow="hidden"
      w="100%"
      {...props}
    >
      <Flex
        alignItems="center"
        bg="$vespertideBg"
        borderBottom="1px solid $border"
        gap="10px"
        px="14px"
        py="10px"
      >
        <Flex gap="6px">
          {[0, 1, 2].map((i) => (
            <Box
              key={i}
              bg="$caption"
              borderRadius="50%"
              boxSize="10px"
              opacity="0.5"
            />
          ))}
        </Flex>
        <Text
          color="$caption"
          fontFamily="D2Coding"
          fontSize="12px"
          ml="4px"
          overflow="hidden"
          textOverflow="ellipsis"
          whiteSpace="nowrap"
        >
          {title}
        </Text>
        {tabs && (
          <Flex gap="2px" ml="auto">
            {tabs}
          </Flex>
        )}
      </Flex>
      <Box overflowX="auto" px="22px" py="20px">
        {children}
      </Box>
    </Box>
  )
}

export function HighlightedCode({ html }: { html: string }) {
  return <div dangerouslySetInnerHTML={{ __html: html }} />
}
