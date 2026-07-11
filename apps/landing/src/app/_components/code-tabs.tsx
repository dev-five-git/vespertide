'use client'

import { Box, Text } from '@devup-ui/react'
import { useState } from 'react'

import { CodeWindow, HighlightedCode } from './code-window'

export interface CodeExample {
  key: string
  label: string
  file: string
  html: string
}

export function CodeTabs({ examples }: { examples: CodeExample[] }) {
  const [active, setActive] = useState(examples[0]?.key ?? '')
  const current = examples.find((e) => e.key === active) ?? examples[0]

  if (!current) return null

  return (
    <CodeWindow
      tabs={examples.map((ex) => (
        <Box
          key={ex.key}
          as="button"
          bg={ex.key === active ? '$containerBackground' : 'transparent'}
          border={ex.key === active ? '1px solid $border' : '1px solid transparent'}
          borderRadius="4px"
          cursor="pointer"
          onClick={() => setActive(ex.key)}
          px="12px"
          py="5px"
          transition="color .15s, background .15s"
        >
          <Text
            color={ex.key === active ? '$title' : '$caption'}
            fontFamily="D2Coding"
            fontSize="11px"
          >
            {ex.label}
          </Text>
        </Box>
      ))}
      title={current.file}
    >
      <HighlightedCode html={current.html} />
    </CodeWindow>
  )
}

