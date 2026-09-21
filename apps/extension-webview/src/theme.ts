import { globalCss, keyframes } from '@devup-ui/react';

globalCss({
  'html, body, #root': { height: '100%', margin: 0, padding: 0 },
  body: { overflow: 'hidden' },
});

export const slideInRight = keyframes({
  from: { transform: 'translateX(20px)', opacity: 0 },
  to: { transform: 'translateX(0)', opacity: 1 },
});

export const slideInUp = keyframes({
  from: { transform: 'translateY(10px)', opacity: 0 },
  to: { transform: 'translateY(0)', opacity: 1 },
});
