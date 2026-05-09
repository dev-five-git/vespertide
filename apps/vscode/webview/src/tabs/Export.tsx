import React, { useState } from 'react';
import { postMessage } from '../vscode';
import type { AppState } from '../App';

type Props = {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
};

type Status = 'idle' | 'loading' | 'done';

type ExportItem = {
  key: string;
  title: string;
  description: string;
  onClick: () => void;
  disabled?: boolean;
};

export default function Export({ state }: Props) {
  const [statuses, setStatuses] = useState<Record<string, Status>>({});

  const setStatus = (key: string, s: Status) =>
    setStatuses((prev) => ({ ...prev, [key]: s }));

  const trigger = (key: string, send: () => void) => {
    setStatus(key, 'loading');
    send();
    // Reset after 3 s (actual completion is reflected via HostMessage)
    setTimeout(() => setStatus(key, 'idle'), 3000);
  };

  const hasErD = !!state.svg;
  const hasSchema = Object.keys(state.schema).length > 0;

  const items: ExportItem[] = [
    {
      key: 'svg',
      title: 'SVG 다운로드',
      description: 'ERD 다이어그램을 SVG 벡터 파일로 저장합니다.',
      onClick: () => trigger('svg', () => postMessage({ type: 'export_svg' })),
      disabled: !hasErD,
    },
    {
      key: 'pdf',
      title: 'PDF 변환',
      description: 'ERD 다이어그램을 PDF 파일로 변환합니다.',
      onClick: () => trigger('pdf', () => postMessage({ type: 'export_pdf' })),
      disabled: !hasErD,
    },
    {
      key: 'mcp',
      title: 'MCP 내보내기',
      description:
        'Schema JSON을 MCP 서버 엔드포인트로 전송합니다. (설정: vespertide.mcpEndpoint)',
      onClick: () =>
        trigger('mcp', () => postMessage({ type: 'export_mcp', schema: state.schema })),
      disabled: !hasSchema,
    },
  ];

  return (
    <div
      style={{
        padding: 16,
        display: 'flex',
        flexDirection: 'column',
        gap: 12,
        overflow: 'auto',
        height: '100%',
      }}
    >
      <p style={{ margin: '0 0 4px', fontSize: 11, opacity: 0.5 }}>
        ERD 및 스키마를 다양한 형식으로 내보냅니다.
      </p>

      {items.map(({ key, title, description, onClick, disabled }) => (
        <div
          key={key}
          style={{
            padding: '14px 16px',
            background: 'var(--vscode-editorWidget-background, #252526)',
            borderRadius: 6,
            border: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 16,
            opacity: disabled ? 0.55 : 1,
          }}
        >
          <div>
            <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 4 }}>{title}</div>
            <div style={{ fontSize: 11, opacity: 0.6, lineHeight: 1.5 }}>{description}</div>
          </div>
          <button
            onClick={onClick}
            disabled={disabled || statuses[key] === 'loading'}
            style={{
              padding: '6px 18px',
              background: 'var(--vscode-button-background, #0e639c)',
              color: 'var(--vscode-button-foreground, #fff)',
              border: 'none',
              borderRadius: 3,
              fontSize: 12,
              cursor: disabled || statuses[key] === 'loading' ? 'not-allowed' : 'pointer',
              whiteSpace: 'nowrap',
              minWidth: 80,
              opacity: disabled || statuses[key] === 'loading' ? 0.6 : 1,
            }}
          >
            {statuses[key] === 'loading' ? '처리 중...' : '내보내기'}
          </button>
        </div>
      ))}

      {!hasErD && !hasSchema && (
        <div
          style={{
            marginTop: 8,
            padding: '10px 14px',
            background:
              'var(--vscode-inputValidation-warningBackground, rgba(255,200,0,0.08))',
            borderRadius: 4,
            border:
              '1px solid var(--vscode-inputValidation-warningBorder, rgba(255,200,0,0.25))',
            fontSize: 12,
            opacity: 0.8,
            lineHeight: 1.6,
          }}
        >
          ORM Editor 탭에서 스키마를 입력하면 내보내기 기능이 활성화됩니다.
        </div>
      )}
    </div>
  );
}
