import { useState } from 'react';

const COPIED_RESET_MS = 1500;

/** 클립보드 복사 + 잠시 동안 "복사됨" 상태(어떤 id가 복사됐는지)를 유지한다. */
export function useCopy() {
  const [copiedId, setCopiedId] = useState<string | null>(null);

  function copy(content: string, id: string) {
    navigator.clipboard
      .writeText(content)
      .then(() => {
        setCopiedId(id);
        setTimeout(() => setCopiedId(null), COPIED_RESET_MS);
      })
      .catch(console.error);
  }

  return { copiedId, copy };
}
