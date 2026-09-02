import { keyframes } from '@devup-ui/react';

export const slideInRight = keyframes({
  from: { transform: 'translateX(20px)', opacity: 0 },
  to: { transform: 'translateX(0)', opacity: 1 },
});

export const slideInUp = keyframes({
  from: { transform: 'translateY(10px)', opacity: 0 },
  to: { transform: 'translateY(0)', opacity: 1 },
});
