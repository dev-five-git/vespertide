'use client'

import { Box, Flex, Text } from '@devup-ui/react'
import { useState } from 'react'

export function CopyInstall({
  command = 'cargo install vespertide-cli',
}: {
  command?: string
}) {
  const [copied, setCopied] = useState(false)

  const copy = () => {
    navigator.clipboard?.writeText(command)
    setCopied(true)
    setTimeout(() => setCopied(false), 1400)
  }

  return (
    <Flex
      _hover={{ borderColor: '$caption' }}
      alignItems="center"
      bg="$vespertideBg"
      border="1px solid $border"
      borderRadius="$borderRadiusRadius08"
      cursor="pointer"
      gap="12px"
      onClick={copy}
      px="$spacingSpacing16"
      py="$spacingSpacing08"
      transition="border-color .15s"
      w="fit-content"
    >
      <Text color="$caption" typography="code">
        $
      </Text>
      <Text color="$title" typography="code">
        {command}
      </Text>
      <Box
        as="button"
        bg="$containerBackground"
        border="1px solid $border"
        borderRadius="4px"
        color="$textSub"
        cursor="pointer"
        fontFamily="D2Coding"
        fontSize="11px"
        onClick={(e) => {
          e.stopPropagation()
          copy()
        }}
        px="9px"
        py="5px"
        transition="color .15s, border-color .15s"
      >
        {copied ? 'copied' : 'copy'}
      </Box>
    </Flex>
  )
}
