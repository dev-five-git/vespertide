import React, { useState } from 'react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';

type Props = {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
};

const ORM_LABELS: Record<OrmType, string> = {
  prisma:     'Prisma',
  typeorm:    'TypeORM',
  drizzle:    'Drizzle',
  jpa:        'JPA',
  sqlalchemy: 'SQLAlchemy',
  gorm:       'GORM',
};

const ORM_TYPES = Object.keys(ORM_LABELS) as OrmType[];

export default function OrmConverter({ state, setState }: Props) {
  const [target, setTarget] = useState<OrmType | null>(null);

  const canConvert = target !== null && target !== state.ormType && !!state.ormSource;

  const handleConvert = () => {
    if (!canConvert || !target) return;
    postMessage({ type: 'convert_orm', source: state.ormSource, from: state.ormType, to: target });
    setState((prev) => ({ ...prev, ormType: target }));
    setTarget(null);
  };

  return (
    <div
      style={{
        padding: 16,
        height: '100%',
        overflow: 'auto',
        display: 'flex',
        flexDirection: 'column',
        gap: 16,
      }}
    >
      {/* ORM buttons */}
      <div>
        <div style={{ fontSize: 11, opacity: 0.55, marginBottom: 8, letterSpacing: '0.03em' }}>
          변환할 대상 ORM 선택
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          {ORM_TYPES.map((orm) => {
            const isCurrent = orm === state.ormType;
            const isTarget  = orm === target;
            return (
              <button
                key={orm}
                onClick={() => !isCurrent && setTarget(isTarget ? null : orm)}
                disabled={isCurrent}
                title={isCurrent ? '현재 ORM' : `${ORM_LABELS[orm]}으로 변환`}
                style={{
                  padding: '6px 14px',
                  border: '1px solid',
                  borderColor: isCurrent
                    ? 'var(--vscode-focusBorder, #007acc)'
                    : isTarget
                    ? 'var(--vscode-charts-green, #4caf50)'
                    : 'var(--vscode-input-border, rgba(255,255,255,0.2))',
                  borderRadius: 4,
                  background: isCurrent
                    ? 'var(--vscode-button-background, #0e639c)'
                    : isTarget
                    ? 'rgba(76,175,80,0.15)'
                    : 'transparent',
                  color: isCurrent
                    ? 'var(--vscode-button-foreground, #fff)'
                    : 'var(--vscode-foreground)',
                  fontSize: 12,
                  opacity: isCurrent ? 1 : 0.9,
                  cursor: isCurrent ? 'not-allowed' : 'pointer',
                  transition: 'all 0.1s',
                }}
              >
                {ORM_LABELS[orm]}
                {isCurrent && (
                  <span style={{ marginLeft: 6, fontSize: 10, opacity: 0.7 }}>현재</span>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* Conversion confirmation bar */}
      {target && target !== state.ormType && (
        <div
          style={{
            padding: '12px 16px',
            borderRadius: 4,
            background: 'var(--vscode-editorWidget-background, #252526)',
            border: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 12,
          }}
        >
          <span style={{ fontSize: 12 }}>
            <strong>{ORM_LABELS[state.ormType]}</strong>
            {' → '}
            <strong>{ORM_LABELS[target]}</strong>
            {'  으로 변환'}
          </span>
          <button
            onClick={handleConvert}
            disabled={!canConvert}
            style={{
              padding: '6px 18px',
              background: canConvert
                ? 'var(--vscode-button-background, #0e639c)'
                : 'var(--vscode-button-secondaryBackground, #3a3a3a)',
              color: 'var(--vscode-button-foreground, #fff)',
              border: 'none',
              borderRadius: 3,
              fontSize: 12,
              cursor: canConvert ? 'pointer' : 'not-allowed',
              opacity: canConvert ? 1 : 0.5,
              whiteSpace: 'nowrap',
            }}
          >
            변환
          </button>
        </div>
      )}

      {!state.ormSource && (
        <p style={{ margin: 0, opacity: 0.5, fontSize: 12 }}>
          ORM Editor 탭에서 먼저 스키마 코드를 입력하세요.
        </p>
      )}

      {/* Current source preview */}
      {state.ormSource && (
        <div style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
          <div style={{ fontSize: 11, opacity: 0.55, marginBottom: 6 }}>
            현재 스키마 ({ORM_LABELS[state.ormType]})
          </div>
          <pre
            style={{
              margin: 0,
              padding: 12,
              flex: 1,
              overflow: 'auto',
              background: 'var(--vscode-editor-background, #1e1e1e)',
              borderRadius: 4,
              fontSize: 12,
              fontFamily:
                'var(--vscode-editor-font-family, Consolas, "Courier New", monospace)',
              color: 'var(--vscode-editor-foreground, #d4d4d4)',
              lineHeight: 1.5,
              whiteSpace: 'pre',
            }}
          >
            {state.ormSource}
          </pre>
        </div>
      )}
    </div>
  );
}
