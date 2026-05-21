import { globalCss } from '@devup-ui/react'

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
