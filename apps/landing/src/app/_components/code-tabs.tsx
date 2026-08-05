import { Box, Text } from '@devup-ui/react'

import { CodeWindow, HighlightedCode } from './code-window'

export interface CodeExample {
  key: string
  label: string
  file: string
  html: string
}

export type CodeExampleQuad = [CodeExample, CodeExample, CodeExample, CodeExample]

// devup-ui extracts `selectors` strings at build time, so each of the 4
// fixed tabs is written out literally (nth-of-type) rather than generated
// by a .map() over a runtime key — a template-literal selector key can't be
// statically extracted into CSS.
export function CodeTabs({ examples: [a, b, c, d] }: { examples: CodeExampleQuad }) {
  return (
    <Box>
      <Box
        as="input"
        boxSize="1px"
        defaultChecked
        id="code-tab-0"
        name="code-tab"
        opacity="0"
        overflow="hidden"
        position="absolute"
        selectors={{
          '&:checked ~ * #code-panel-0': { display: 'block' },
          '&:checked ~ * #code-title-0': { display: 'block' },
          '&:checked ~ * label:nth-of-type(1)': {
            bg: '$containerBackground',
            border: '1px solid $border',
            color: '$title',
          },
          '&:focus-visible ~ * label:nth-of-type(1)': {
            boxShadow: '0 0 0 2px $vespertidePrimary',
          },
        }}
        type="radio"
      />
      <Box
        as="input"
        boxSize="1px"
        id="code-tab-1"
        name="code-tab"
        opacity="0"
        overflow="hidden"
        position="absolute"
        selectors={{
          '&:checked ~ * #code-panel-1': { display: 'block' },
          '&:checked ~ * #code-title-1': { display: 'block' },
          '&:checked ~ * label:nth-of-type(2)': {
            bg: '$containerBackground',
            border: '1px solid $border',
            color: '$title',
          },
          '&:focus-visible ~ * label:nth-of-type(2)': {
            boxShadow: '0 0 0 2px $vespertidePrimary',
          },
        }}
        type="radio"
      />
      <Box
        as="input"
        boxSize="1px"
        id="code-tab-2"
        name="code-tab"
        opacity="0"
        overflow="hidden"
        position="absolute"
        selectors={{
          '&:checked ~ * #code-panel-2': { display: 'block' },
          '&:checked ~ * #code-title-2': { display: 'block' },
          '&:checked ~ * label:nth-of-type(3)': {
            bg: '$containerBackground',
            border: '1px solid $border',
            color: '$title',
          },
          '&:focus-visible ~ * label:nth-of-type(3)': {
            boxShadow: '0 0 0 2px $vespertidePrimary',
          },
        }}
        type="radio"
      />
      <Box
        as="input"
        boxSize="1px"
        id="code-tab-3"
        name="code-tab"
        opacity="0"
        overflow="hidden"
        position="absolute"
        selectors={{
          '&:checked ~ * #code-panel-3': { display: 'block' },
          '&:checked ~ * #code-title-3': { display: 'block' },
          '&:checked ~ * label:nth-of-type(4)': {
            bg: '$containerBackground',
            border: '1px solid $border',
            color: '$title',
          },
          '&:focus-visible ~ * label:nth-of-type(4)': {
            boxShadow: '0 0 0 2px $vespertidePrimary',
          },
        }}
        type="radio"
      />
      <CodeWindow
        tabs={
          <>
            <Box
              as="label"
              border="1px solid transparent"
              borderRadius="4px"
              color="$caption"
              cursor="pointer"
              fontFamily="D2Coding"
              fontSize="11px"
              htmlFor="code-tab-0"
              px="12px"
              py="5px"
              transition="color .15s, background .15s"
            >
              {a.label}
            </Box>
            <Box
              as="label"
              border="1px solid transparent"
              borderRadius="4px"
              color="$caption"
              cursor="pointer"
              fontFamily="D2Coding"
              fontSize="11px"
              htmlFor="code-tab-1"
              px="12px"
              py="5px"
              transition="color .15s, background .15s"
            >
              {b.label}
            </Box>
            <Box
              as="label"
              border="1px solid transparent"
              borderRadius="4px"
              color="$caption"
              cursor="pointer"
              fontFamily="D2Coding"
              fontSize="11px"
              htmlFor="code-tab-2"
              px="12px"
              py="5px"
              transition="color .15s, background .15s"
            >
              {c.label}
            </Box>
            <Box
              as="label"
              border="1px solid transparent"
              borderRadius="4px"
              color="$caption"
              cursor="pointer"
              fontFamily="D2Coding"
              fontSize="11px"
              htmlFor="code-tab-3"
              px="12px"
              py="5px"
              transition="color .15s, background .15s"
            >
              {d.label}
            </Box>
          </>
        }
        title={
          <>
            <Text as="span" display="block" id="code-title-0">
              {a.file}
            </Text>
            <Text as="span" display="none" id="code-title-1">
              {b.file}
            </Text>
            <Text as="span" display="none" id="code-title-2">
              {c.file}
            </Text>
            <Text as="span" display="none" id="code-title-3">
              {d.file}
            </Text>
          </>
        }
      >
        <Box display="block" id="code-panel-0">
          <HighlightedCode html={a.html} />
        </Box>
        <Box display="none" id="code-panel-1">
          <HighlightedCode html={b.html} />
        </Box>
        <Box display="none" id="code-panel-2">
          <HighlightedCode html={c.html} />
        </Box>
        <Box display="none" id="code-panel-3">
          <HighlightedCode html={d.html} />
        </Box>
      </CodeWindow>
    </Box>
  )
}