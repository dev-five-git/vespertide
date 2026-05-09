import React, { useState, useEffect, useRef, useCallback } from 'react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';

// ── Types ────────────────────────────────────────────────────────────────────

type Field = {
  name: string;
  type: string;
  isPrimary: boolean;
  isRelation: boolean;
  decorators: string;
};

type Model = {
  name: string;
  fields: Field[];
};

type Pos = { x: number; y: number };

// ── ORM Parsers ───────────────────────────────────────────────────────────────

function parsePrisma(src: string): Model[] {
  const models: Model[] = [];
  const re = /model\s+(\w+)\s*\{([^}]+)\}/g;
  let m;
  while ((m = re.exec(src)) !== null) {
    const name = m[1];
    const fields: Field[] = m[2]
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith('//') && !l.startsWith('@@'))
      .map((l) => {
        const parts = l.split(/\s+/);
        const rest = parts.slice(2).join(' ');
        const baseType = parts[1]?.replace('[]', '').replace('?', '') ?? '';
        return {
          name: parts[0],
          type: parts[1] ?? '',
          isPrimary: rest.includes('@id'),
          isRelation: /^[A-Z]/.test(baseType) && baseType !== 'String' && baseType !== 'Boolean',
          decorators: rest,
        };
      });
    models.push({ name, fields });
  }
  return models;
}

function parseSource(src: string, orm: OrmType): Model[] {
  if (!src.trim()) return [];
  if (orm === 'prisma') return parsePrisma(src);
  return []; // other ORMs: handled by WASM when connected
}

// ── Utilities ─────────────────────────────────────────────────────────────────

const PALETTE = ['#6366f1', '#8b5cf6', '#10b981', '#f59e0b', '#ef4444', '#06b6d4', '#ec4899', '#84cc16'];

function modelColor(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffff;
  return PALETTE[h % PALETTE.length];
}

function prismaTypeToJson(t: string): string {
  const base = t.replace('?', '').replace('[]', '');
  const map: Record<string, string> = {
    Int: 'integer', BigInt: 'bigint', Float: 'float', Decimal: 'decimal',
    String: 'varchar', Boolean: 'boolean', DateTime: 'timestamp',
    Json: 'json', Bytes: 'bytea',
  };
  return map[base] ?? base.toLowerCase();
}

function modelToJson(model: Model): string {
  return JSON.stringify(
    {
      name: model.name,
      columns: model.fields
        .filter((f) => !f.isRelation)
        .map((f) => ({
          name: f.name,
          type: prismaTypeToJson(f.type),
          nullable: f.type.endsWith('?'),
          ...(f.isPrimary ? { primary_key: { auto_increment: true } } : {}),
        })),
      relations: model.fields
        .filter((f) => f.isRelation)
        .map((f) => ({ field: f.name, references: f.type.replace('[]', '').replace('?', '') })),
    },
    null,
    2
  );
}

function getEdges(models: Model[]): { from: string; to: string }[] {
  const names = new Set(models.map((m) => m.name));
  const edges: { from: string; to: string }[] = [];
  for (const model of models) {
    for (const field of model.fields) {
      const base = field.type.replace('[]', '').replace('?', '');
      if (field.isRelation && names.has(base) && base !== model.name) {
        edges.push({ from: model.name, to: base });
      }
    }
  }
  return edges;
}

// ── Constants ─────────────────────────────────────────────────────────────────

const ORM_TYPES: OrmType[] = ['prisma', 'typeorm', 'drizzle', 'jpa', 'sqlalchemy', 'gorm'];
const NODE_W = 190;
const HEADER_H = 36;
const FIELD_H = 22;
const NODE_PAD_B = 6;

function nodeHeight(model: Model) {
  return HEADER_H + model.fields.length * FIELD_H + NODE_PAD_B;
}

// ── Component ─────────────────────────────────────────────────────────────────

type Props = {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
};

export default function OrmEditor({ state, setState }: Props) {
  const [models, setModels] = useState<Model[]>([]);
  const [positions, setPositions] = useState<Record<string, Pos>>({});
  const [selected, setSelected] = useState<Model | null>(null);
  const [showCode, setShowCode] = useState(false);
  const [pan, setPan] = useState<Pos>({ x: 32, y: 32 });

  const draggingRef = useRef<{ id: string; ox: number; oy: number } | null>(null);
  const panningRef = useRef<{ mx: number; my: number; px: number; py: number } | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const canvasRef = useRef<HTMLDivElement>(null);

  // Parse on source / ormType change
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      const parsed = parseSource(state.ormSource, state.ormType);
      setModels(parsed);
      setPositions((prev) => {
        const next = { ...prev };
        parsed.forEach((m, i) => {
          if (!next[m.name]) {
            next[m.name] = { x: 40 + (i % 3) * 230, y: 40 + Math.floor(i / 3) * 210 };
          }
        });
        // Remove stale positions
        const names = new Set(parsed.map((m) => m.name));
        for (const k of Object.keys(next)) {
          if (!names.has(k)) delete next[k];
        }
        return next;
      });
      if (state.ormSource) {
        postMessage({ type: 'parse_orm', source: state.ormSource, orm: state.ormType });
      }
    }, 300);
  }, [state.ormSource, state.ormType]);

  // Global mouse handlers
  const onMouseMove = useCallback((e: MouseEvent) => {
    if (draggingRef.current) {
      const { id, ox, oy } = draggingRef.current;
      setPositions((prev) => ({ ...prev, [id]: { x: e.clientX - ox, y: e.clientY - oy } }));
    } else if (panningRef.current) {
      const { mx, my, px, py } = panningRef.current;
      setPan({ x: px + e.clientX - mx, y: py + e.clientY - my });
    }
  }, []);

  const onMouseUp = useCallback(() => {
    draggingRef.current = null;
    panningRef.current = null;
  }, []);

  useEffect(() => {
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
    return () => {
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };
  }, [onMouseMove, onMouseUp]);

  const startNodeDrag = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    const pos = positions[id] ?? { x: 0, y: 0 };
    draggingRef.current = { id, ox: e.clientX - pos.x - pan.x, oy: e.clientY - pos.y - pan.y };
  };

  const startPan = (e: React.MouseEvent) => {
    if (e.target === canvasRef.current || (e.target as HTMLElement).dataset.bg) {
      panningRef.current = { mx: e.clientX, my: e.clientY, px: pan.x, py: pan.y };
    }
  };

  const edges = getEdges(models);

  // Edge bezier paths
  const renderEdges = () =>
    edges.map((edge, i) => {
      const from = positions[edge.from];
      const to = positions[edge.to];
      const fromModel = models.find((m) => m.name === edge.from);
      const toModel = models.find((m) => m.name === edge.to);
      if (!from || !to || !fromModel || !toModel) return null;

      const x1 = from.x + pan.x + NODE_W;
      const y1 = from.y + pan.y + HEADER_H / 2;
      const x2 = to.x + pan.x;
      const y2 = to.y + pan.y + HEADER_H / 2;
      const cx = (x1 + x2) / 2;

      return (
        <g key={i}>
          <path
            d={`M${x1} ${y1} C${cx} ${y1},${cx} ${y2},${x2} ${y2}`}
            fill="none"
            stroke="rgba(99,102,241,0.5)"
            strokeWidth="1.5"
            strokeDasharray="5 3"
          />
          <circle cx={x2} cy={y2} r={3.5} fill="rgba(99,102,241,0.8)" />
          <circle cx={x1} cy={y1} r={3} fill="rgba(99,102,241,0.5)" />
        </g>
      );
    });

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* ── Toolbar ── */}
      <div style={{
        display: 'flex', gap: 4, padding: '6px 8px', flexShrink: 0, flexWrap: 'wrap',
        alignItems: 'center',
        background: 'var(--vscode-editorWidget-background, #252526)',
        borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
      }}>
        {ORM_TYPES.map((orm) => (
          <button key={orm}
            onClick={() => setState((p) => ({ ...p, ormType: orm }))}
            style={{
              padding: '2px 10px', border: '1px solid', borderRadius: 3, fontSize: 11,
              borderColor: state.ormType === orm ? 'var(--vscode-focusBorder,#007acc)' : 'rgba(255,255,255,0.15)',
              background: state.ormType === orm ? 'var(--vscode-button-background,#0e639c)' : 'transparent',
              color: state.ormType === orm ? 'var(--vscode-button-foreground,#fff)' : 'var(--vscode-foreground)',
              cursor: 'pointer',
            }}
          >{orm}</button>
        ))}
        <div style={{ flex: 1 }} />
        <button
          onClick={() => setShowCode((v) => !v)}
          style={{
            padding: '2px 10px', border: '1px solid', borderRadius: 3, fontSize: 11, cursor: 'pointer',
            borderColor: showCode ? 'var(--vscode-focusBorder,#007acc)' : 'rgba(255,255,255,0.15)',
            background: showCode ? 'rgba(0,122,204,0.15)' : 'transparent',
            color: 'var(--vscode-foreground)',
          }}
        >{'</>'} Code</button>
      </div>

      {/* ── Canvas + Detail panel ── */}
      <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>
        {/* Canvas */}
        <div
          ref={canvasRef}
          onMouseDown={startPan}
          onClick={() => setSelected(null)}
          style={{
            flex: 1, position: 'relative', overflow: 'hidden',
            cursor: panningRef.current ? 'grabbing' : 'grab',
            background: 'var(--vscode-editor-background, #1e1e1e)',
          }}
        >
          {/* Dot-grid background */}
          <div
            data-bg="true"
            style={{
              position: 'absolute', inset: 0, pointerEvents: 'none',
              backgroundImage: 'radial-gradient(circle, rgba(255,255,255,0.07) 1px, transparent 1px)',
              backgroundSize: '24px 24px',
              backgroundPosition: `${pan.x % 24}px ${pan.y % 24}px`,
            }}
          />

          {/* SVG edges */}
          <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none', overflow: 'visible' }}>
            {renderEdges()}
          </svg>

          {/* Nodes */}
          {models.map((model) => {
            const pos = positions[model.name] ?? { x: 0, y: 0 };
            const color = modelColor(model.name);
            const isSelected = selected?.name === model.name;
            const h = nodeHeight(model);

            return (
              <div
                key={model.name}
                onMouseDown={(e) => startNodeDrag(e, model.name)}
                onClick={(e) => { e.stopPropagation(); setSelected(isSelected ? null : model); }}
                style={{
                  position: 'absolute',
                  left: pos.x + pan.x,
                  top: pos.y + pan.y,
                  width: NODE_W,
                  height: h,
                  borderRadius: 8,
                  border: `1.5px solid ${isSelected ? color : 'rgba(255,255,255,0.1)'}`,
                  boxShadow: isSelected
                    ? `0 0 0 3px ${color}30, 0 4px 16px rgba(0,0,0,0.4)`
                    : '0 2px 10px rgba(0,0,0,0.35)',
                  background: 'var(--vscode-editor-background, #1e1e1e)',
                  overflow: 'hidden',
                  userSelect: 'none',
                  cursor: 'pointer',
                  transition: 'border-color 0.12s, box-shadow 0.12s',
                }}
              >
                {/* Header */}
                <div style={{
                  height: HEADER_H, display: 'flex', alignItems: 'center',
                  padding: '0 12px', gap: 8,
                  background: `${color}1a`,
                  borderBottom: `1px solid ${color}30`,
                }}>
                  <div style={{ width: 9, height: 9, borderRadius: '50%', background: color, flexShrink: 0 }} />
                  <span style={{ fontWeight: 700, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {model.name}
                  </span>
                </div>

                {/* Fields */}
                {model.fields.map((field, fi) => (
                  <div key={field.name} style={{
                    height: FIELD_H, display: 'flex', alignItems: 'center',
                    padding: '0 12px', gap: 6,
                    borderBottom: fi < model.fields.length - 1 ? '1px solid rgba(255,255,255,0.04)' : 'none',
                  }}>
                    <span style={{ fontSize: 9, width: 10, flexShrink: 0, opacity: 0.5, textAlign: 'center' }}>
                      {field.isPrimary ? '⬡' : field.isRelation ? '↗' : '·'}
                    </span>
                    <span style={{ fontSize: 11, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {field.name}
                    </span>
                    <span style={{
                      fontSize: 10, flexShrink: 0, opacity: 0.4,
                      fontFamily: 'var(--vscode-editor-font-family, monospace)',
                      color: field.isRelation ? color : 'inherit',
                    }}>
                      {field.type}
                    </span>
                  </div>
                ))}
              </div>
            );
          })}

          {/* Empty state */}
          {models.length === 0 && (
            <div style={{
              position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column',
              alignItems: 'center', justifyContent: 'center', gap: 8,
              opacity: 0.3, pointerEvents: 'none',
            }}>
              <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1">
                <rect x="3" y="3" width="18" height="18" rx="2"/>
                <line x1="3" y1="9" x2="21" y2="9"/>
                <line x1="9" y1="9" x2="9" y2="21"/>
              </svg>
              <div style={{ fontSize: 12 }}>&lt;/&gt; Code 버튼을 눌러 ORM 스키마를 입력하세요</div>
            </div>
          )}
        </div>

        {/* ── Detail panel (slide in) ── */}
        {selected && (
          <div style={{
            width: 270, flexShrink: 0,
            borderLeft: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            display: 'flex', flexDirection: 'column',
            background: 'var(--vscode-sideBar-background, #252526)',
            animation: 'slideInRight 0.15s ease-out',
          }}>
            {/* Panel header */}
            <div style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              padding: '10px 14px', flexShrink: 0,
              borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <div style={{ width: 10, height: 10, borderRadius: '50%', background: modelColor(selected.name) }} />
                <span style={{ fontWeight: 700, fontSize: 13 }}>{selected.name}</span>
              </div>
              <button
                onClick={(e) => { e.stopPropagation(); setSelected(null); }}
                style={{ background: 'none', border: 'none', color: 'var(--vscode-foreground)', fontSize: 14, cursor: 'pointer', opacity: 0.5, lineHeight: 1, padding: 2 }}
              >✕</button>
            </div>

            {/* Fields summary */}
            <div style={{ padding: '10px 14px', flexShrink: 0, borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
              {selected.fields.map((f) => (
                <div key={f.name} style={{ display: 'flex', alignItems: 'baseline', gap: 6, marginBottom: 4 }}>
                  <span style={{ fontSize: 10, opacity: 0.4, width: 12, textAlign: 'center' }}>
                    {f.isPrimary ? '⬡' : f.isRelation ? '↗' : '·'}
                  </span>
                  <span style={{ fontSize: 12, flex: 1 }}>{f.name}</span>
                  <span style={{
                    fontSize: 10, opacity: 0.45,
                    fontFamily: 'monospace',
                    color: f.isRelation ? modelColor(f.type.replace('[]','').replace('?','')) : 'inherit',
                  }}>{f.type}</span>
                </div>
              ))}
            </div>

            {/* JSON output */}
            <div style={{ padding: '8px 14px 4px', fontSize: 10, opacity: 0.4, flexShrink: 0, letterSpacing: '0.05em' }}>
              EXPORT JSON
            </div>
            <pre style={{
              flex: 1, margin: 0, padding: '0 14px 14px', overflow: 'auto',
              fontFamily: 'var(--vscode-editor-font-family, Consolas, monospace)',
              fontSize: 11, lineHeight: 1.65,
              color: 'var(--vscode-editor-foreground, #d4d4d4)',
              background: 'transparent',
              whiteSpace: 'pre',
            }}>
              {modelToJson(selected)}
            </pre>
          </div>
        )}
      </div>

      {/* ── Code drawer (bottom) ── */}
      {showCode && (
        <div style={{
          height: 200, flexShrink: 0,
          borderTop: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
          display: 'flex', flexDirection: 'column',
          animation: 'slideInUp 0.15s ease-out',
        }}>
          <div style={{
            padding: '4px 12px', fontSize: 10, opacity: 0.45, flexShrink: 0,
            borderBottom: '1px solid rgba(255,255,255,0.05)',
            display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          }}>
            <span>ORM 소스 코드</span>
            <span style={{ opacity: 0.6 }}>{state.ormType}</span>
          </div>
          <textarea
            value={state.ormSource}
            onChange={(e) => setState((p) => ({ ...p, ormSource: e.target.value }))}
            spellCheck={false}
            placeholder={state.ormType === 'prisma'
              ? `model User {\n  id    Int    @id @default(autoincrement())\n  name  String\n  posts Post[]\n}`
              : `${state.ormType} 스키마를 입력하세요 (WASM 연결 후 파싱 지원)`}
            style={{
              flex: 1, resize: 'none', border: 'none', outline: 'none',
              padding: '8px 12px',
              fontFamily: 'var(--vscode-editor-font-family, Consolas, monospace)',
              fontSize: 12,
              color: 'var(--vscode-editor-foreground, #d4d4d4)',
              background: 'var(--vscode-editor-background, #1e1e1e)',
              lineHeight: 1.6,
            }}
          />
        </div>
      )}
    </div>
  );
}
