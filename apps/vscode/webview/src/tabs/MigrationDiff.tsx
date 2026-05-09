import React, { useEffect, useRef } from 'react';
import { postMessage } from '../vscode';
import type { AppState } from '../App';

type Props = {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
};

type Column = { label: string; sql: string };

export default function MigrationDiff({ state, setState: _setState }: Props) {
  const lastSchemaRef = useRef<string>('');

  // Re-generate when schema changes (or on first mount if schema exists)
  useEffect(() => {
    const key = JSON.stringify(state.schema);
    if (key === lastSchemaRef.current) return;
    if (!Object.keys(state.schema).length) return;
    lastSchemaRef.current = key;

    // Send one message; host will call generateMigration for all 3 dialects in parallel
    postMessage({ type: 'generate_migration', schema: state.schema, db: 'postgres' });
  }, [state.schema]);

  const columns: Column[] = [
    { label: 'PostgreSQL', sql: state.postgres },
    { label: 'MySQL',      sql: state.mysql },
    { label: 'SQLite',     sql: state.sqlite },
  ];

  const copy = (text: string) => navigator.clipboard.writeText(text).catch(console.error);

  const empty = !state.postgres && !state.mysql && !state.sqlite;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div
        style={{
          padding: '7px 12px',
          fontSize: 11,
          opacity: 0.55,
          borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
          flexShrink: 0,
        }}
      >
        현재 스키마 기준 마이그레이션 SQL — 3개 DB 방언 비교
      </div>

      {empty && (
        <div
          style={{
            flex: 1,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            opacity: 0.35,
            fontSize: 12,
            textAlign: 'center',
            lineHeight: 1.8,
          }}
        >
          <div>
            <div>ORM Editor 탭에서 스키마를 입력하면</div>
            <div>마이그레이션 SQL이 여기에 표시됩니다</div>
          </div>
        </div>
      )}

      {/* 3-column side-by-side */}
      {!empty && (
        <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
          {columns.map(({ label, sql }, i) => (
            <React.Fragment key={label}>
              {i > 0 && (
                <div
                  style={{
                    width: 1,
                    background: 'var(--vscode-panel-border, rgba(255,255,255,0.1))',
                    flexShrink: 0,
                  }}
                />
              )}
              <div
                style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}
              >
                {/* Column header */}
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    padding: '5px 10px',
                    background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
                    borderBottom:
                      '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
                    flexShrink: 0,
                  }}
                >
                  <span style={{ fontSize: 11, fontWeight: 600 }}>{label}</span>
                  {sql && (
                    <button
                      onClick={() => copy(sql)}
                      title="클립보드에 복사"
                      style={{
                        background: 'transparent',
                        border: '1px solid var(--vscode-input-border, rgba(255,255,255,0.2))',
                        borderRadius: 3,
                        color: 'var(--vscode-foreground)',
                        padding: '1px 8px',
                        fontSize: 10,
                        cursor: 'pointer',
                      }}
                    >
                      복사
                    </button>
                  )}
                </div>

                {/* SQL body */}
                <pre
                  style={{
                    margin: 0,
                    padding: '10px 12px',
                    flex: 1,
                    overflow: 'auto',
                    fontFamily:
                      'var(--vscode-editor-font-family, Consolas, "Courier New", monospace)',
                    fontSize: 11,
                    color: 'var(--vscode-editor-foreground, #d4d4d4)',
                    background: 'var(--vscode-editor-background, #1e1e1e)',
                    lineHeight: 1.55,
                    whiteSpace: 'pre',
                  }}
                >
                  {sql || '—'}
                </pre>
              </div>
            </React.Fragment>
          ))}
        </div>
      )}
    </div>
  );
}
