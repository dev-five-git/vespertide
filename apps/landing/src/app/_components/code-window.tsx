import { Box, Flex, globalCss, Text } from '@devup-ui/react'
import type { ComponentProps, ReactNode } from 'react'

globalCss({
  '.shiki, .shiki span': {
    fontFamily: 'D2Coding',
    fontSize: '13px',
    lineHeight: '1.65',
  },
  '.shiki': {
    background: 'transparent !important',
    padding: '0',
    margin: '0',
    overflowX: 'auto',
  },
  '[data-theme="dark"] .shiki, [data-theme="dark"] .shiki span': {
    color: 'var(--shiki-dark) !important',
    backgroundColor: 'transparent !important',
  },
})

export function CodeWindow({
  title,
  tabs,
  children,
  ...props
}: {
  title: ReactNode
  tabs?: ReactNode
  children: ReactNode
} & Omit<ComponentProps<typeof Box<'div'>>, 'title'>) {
  return (
    <Box
      bg="$cardBase"
      border="1px solid $border"
      borderRadius="$borderRadiusRadius12"
      boxShadow="0 30px 60px -30px rgba(0,0,0,.25)"
      overflow="hidden"
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
          {Array.from({ length: 3 }, (_, i) => (
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

export function StaticCodeBlock({
  title,
  html,
}: {
  title: string
  html: string
}) {
  return (
    <CodeWindow title={title}>
      <HighlightedCode html={html} />
    </CodeWindow>
  )
}

export function HeroCodeWrapper({ children }: { children: ReactNode }) {
  return (
    <Flex justifyContent={[null, null, null, 'flex-end']} w="100%">
      {children}
    </Flex>
  )
}
