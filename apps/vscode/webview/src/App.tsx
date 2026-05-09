import React, { useState, useEffect } from 'react';
import { onMessage } from './vscode';
import type { HostMessage, OrmType, Schema } from './vscode';
import OrmEditor from './tabs/OrmEditor';
import OrmConverter from './tabs/OrmConverter';
import MigrationDiff from './tabs/MigrationDiff';
import Export from './tabs/Export';

type Tab = 'editor' | 'converter' | 'migration' | 'export';

const TABS: { id: Tab; label: string }[] = [
  { id: 'editor',    label: 'ORM Editor' },
  { id: 'converter', label: 'Converter' },
  { id: 'migration', label: 'Migration' },
  { id: 'export',    label: 'Export' },
];

export type AppState = {
  ormSource: string;
  ormType: OrmType;
  svg: string;
  schema: Schema;
  postgres: string;
  mysql: string;
  sqlite: string;
  error: string | null;
};

const INITIAL: AppState = {
  ormSource: '',
  ormType: 'prisma',
  svg: '',
  schema: {},
  postgres: '',
  mysql: '',
  sqlite: '',
  error: null,
};

export default function App() {
  const [tab, setTab] = useState<Tab>('editor');
  const [state, setState] = useState<AppState>(INITIAL);

  useEffect(() => {
    return onMessage((msg: HostMessage) => {
      setState((prev) => {
        switch (msg.type) {
          case 'erd_updated':
            return { ...prev, svg: msg.svg, error: null };
          case 'orm_converted':
            return { ...prev, ormSource: msg.source, error: null };
          case 'migration_updated':
            return {
              ...prev,
              postgres: msg.postgres,
              mysql: msg.mysql,
              sqlite: msg.sqlite,
              error: null,
            };
          case 'export_done':
            return { ...prev, error: null };
          case 'error':
            return { ...prev, error: msg.message };
          default:
            return prev;
        }
      });
    });
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh' }}>
      {/* ── Tab bar ── */}
      <div
        role="tablist"
        style={{
          display: 'flex',
          borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
          background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
          flexShrink: 0,
        }}
      >
        {TABS.map(({ id, label }) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            style={{
              flex: 1,
              padding: '8px 4px',
              border: 'none',
              borderBottom: tab === id
                ? '2px solid var(--vscode-focusBorder, #007acc)'
                : '2px solid transparent',
              background: 'transparent',
              color: tab === id
                ? 'var(--vscode-foreground)'
                : 'var(--vscode-tab-inactiveForeground, #8e8e8e)',
              fontSize: '11px',
              fontWeight: tab === id ? 600 : 400,
              cursor: 'pointer',
              transition: 'color 0.1s, border-color 0.1s',
            }}
          >
            {label}
          </button>
        ))}
      </div>

      {/* ── Error banner ── */}
      {state.error && (
        <div
          style={{
            padding: '6px 12px',
            background: 'var(--vscode-inputValidation-errorBackground, rgba(90,29,29,0.9))',
            color: 'var(--vscode-inputValidation-errorForeground, #f48771)',
            fontSize: '12px',
            flexShrink: 0,
          }}
        >
          {state.error}
        </div>
      )}

      {/* ── Tab content ── */}
      <div style={{ flex: 1, overflow: 'hidden' }}>
        {tab === 'editor'    && <OrmEditor    state={state} setState={setState} />}
        {tab === 'converter' && <OrmConverter state={state} setState={setState} />}
        {tab === 'migration' && <MigrationDiff state={state} setState={setState} />}
        {tab === 'export'    && <Export       state={state} setState={setState} />}
      </div>
    </div>
  );
}
