import React, { useState, useEffect, useRef, useCallback } from 'react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';
import { DEFAULT_SCHEMAS } from '../App';

// ── Types ─────────────────────────────────────────────────────────────────────

type Field = { name: string; type: string; isPrimary: boolean; isRelation: boolean };
type Model = { name: string; fields: Field[] };
type Pos   = { x: number; y: number };

// ── Prisma parser ─────────────────────────────────────────────────────────────

const PRISMA_SCALARS = new Set([
  'Int', 'BigInt', 'Float', 'Decimal', 'String', 'Boolean',
  'DateTime', 'Json', 'Bytes', 'ID',
]);

function parsePrisma(src: string): Model[] {
  const models: Model[] = [];
  const re = /model\s+(\w+)\s*\{([^}]+)\}/g;
  let m;
  while ((m = re.exec(src)) !== null) {
    const fields: Field[] = m[2]
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith('//') && !l.startsWith('@@'))
      .map((l) => {
        const parts = l.split(/\s+/);
        const base  = (parts[1] ?? '').replace('[]', '').replace('?', '');
        return {
          name:       parts[0],
          type:       parts[1] ?? '',
          isPrimary:  l.includes('@id'),
          isRelation: !PRISMA_SCALARS.has(base) && /^[A-Z]/.test(base),
        };
      });
    models.push({ name: m[1], fields });
  }
  return models;
}

function parseSource(src: string, orm: OrmType): Model[] {
  if (!src.trim()) return [];
  return orm === 'prisma' ? parsePrisma(src) : [];
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const TYPE_MAP: Record<string, string> = {
  Int: 'integer', BigInt: 'bigint', Float: 'float', Decimal: 'decimal',
  String: 'varchar', Boolean: 'boolean', DateTime: 'timestamp', Json: 'json', Bytes: 'bytea',
};

function modelToJson(model: Model, ormType: OrmType) {
  return {
    orm:     ormType,
    name:    model.name,
    columns: model.fields
      .filter((f) => !f.isRelation)
      .map((f) => ({
        name:     f.name,
        type:     TYPE_MAP[f.type.replace('?', '').replace('[]', '')] ?? f.type.toLowerCase(),
        nullable: f.type.endsWith('?'),
        ...(f.isPrimary ? { primary_key: { auto_increment: true } } : {}),
      })),
    relations: model.fields
      .filter((f) => f.isRelation)
      .map((f) => ({
        field:      f.name,
        references: f.type.replace('[]', '').replace('?', ''),
        type:       f.type.endsWith('[]') ? 'one-to-many' : 'many-to-one',
      })),
  };
}

function getEdges(models: Model[]): { from: string; to: string }[] {
  const names = new Set(models.map((m) => m.name));
  const seen  = new Set<string>();
  const edges: { from: string; to: string }[] = [];
  for (const model of models) {
    for (const f of model.fields) {
      const base = f.type.replace('[]', '').replace('?', '');
      if (f.isRelation && names.has(base)) {
        const key = [model.name, base].sort().join(':');
        if (!seen.has(key)) { seen.add(key); edges.push({ from: model.name, to: base }); }
      }
    }
  }
  return edges;
}

const PALETTE = ['#6366f1','#8b5cf6','#10b981','#f59e0b','#ef4444','#06b6d4','#ec4899','#84cc16'];
function modelColor(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) & 0xffff;
  return PALETTE[h % PALETTE.length];
}

const ORM_TYPES: OrmType[] = ['prisma', 'typeorm', 'drizzle', 'jpa', 'sqlalchemy', 'gorm'];
const NODE_W        = 204;
const HEADER_H      = 38;
const FIELD_H       = 23;
const DRAG_THRESHOLD = 5;

function nodeHeight(m: Model) { return HEADER_H + m.fields.length * FIELD_H + 6; }

// ── Button style ──────────────────────────────────────────────────────────────

const btnBase: React.CSSProperties = {
  padding: '2px 8px', border: '1px solid rgba(255,255,255,0.15)', borderRadius: 3,
  background: 'transparent', color: 'var(--vscode-foreground)', fontSize: 11, cursor: 'pointer',
};

// ── Component ─────────────────────────────────────────────────────────────────

type Props = { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> };

export default function OrmEditor({ state, setState }: Props) {
  const [models,   setModels]   = useState<Model[]>([]);
  const [positions, setPositions] = useState<Record<string, Pos>>({});
  const [selected, setSelected] = useState<Model | null>(null);
  const [showCode, setShowCode] = useState(false);
  const [pan,   setPan]   = useState<Pos>({ x: 32, y: 32 });
  const [scale, setScale] = useState(1);

  // Keep refs in sync so event handlers never go stale
  const panRef   = useRef(pan);
  const scaleRef = useRef(scale);
  useEffect(() => { panRef.current = pan; },   [pan]);
  useEffect(() => { scaleRef.current = scale; }, [scale]);

  // Drag / pan state
  const pendingRef  = useRef<{ id: string; ox: number; oy: number; sx: number; sy: number } | null>(null);
  const draggingRef = useRef<{ id: string; ox: number; oy: number } | null>(null);
  const didDragRef  = useRef(false);
  const panningRef  = useRef<{ mx: number; my: number; px: number; py: number } | null>(null);

  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const canvasRef   = useRef<HTMLDivElement>(null);

  // ── Parse ORM source ────────────────────────────────────────────────────────

  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      const parsed = parseSource(state.ormSource, state.ormType);
      setModels(parsed);
      setPositions((prev) => {
        const next = { ...prev };
        parsed.forEach((m, i) => {
          if (!next[m.name]) next[m.name] = { x: 40 + (i % 3) * 254, y: 40 + Math.floor(i / 3) * 220 };
        });
        const alive = new Set(parsed.map((m) => m.name));
        for (const k of Object.keys(next)) if (!alive.has(k)) delete next[k];
        return next;
      });
      if (state.ormSource) postMessage({ type: 'parse_orm', source: state.ormSource, orm: state.ormType });
    }, 300);
  }, [state.ormSource, state.ormType]);

  // Keep selected model fresh when models re-parse
  useEffect(() => {
    if (!selected) return;
    const fresh = models.find((m) => m.name === selected.name);
    setSelected(fresh ?? null);
  }, [models]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Mouse handlers ───────────────────────────────────────────────────────────

  const onMouseMove = useCallback((e: MouseEvent) => {
    const { x: px, y: py } = panRef.current;
    const sc = scaleRef.current;

    // Promote pending → dragging once threshold exceeded
    if (pendingRef.current && !draggingRef.current) {
      const dx = Math.abs(e.clientX - pendingRef.current.sx);
      const dy = Math.abs(e.clientY - pendingRef.current.sy);
      if (dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD) {
        draggingRef.current = pendingRef.current;
        didDragRef.current  = true;
        pendingRef.current  = null;
      }
    }

    if (draggingRef.current) {
      const { id, ox, oy } = draggingRef.current;
      setPositions((prev) => ({
        ...prev,
        [id]: { x: (e.clientX - px) / sc - ox, y: (e.clientY - py) / sc - oy },
      }));
    } else if (panningRef.current) {
      const { mx, my, px: ppx, py: ppy } = panningRef.current;
      setPan({ x: ppx + e.clientX - mx, y: ppy + e.clientY - my });
    }
  }, []);

  const onMouseUp = useCallback(() => {
    pendingRef.current  = null;
    draggingRef.current = null;
    panningRef.current  = null;
  }, []);

  useEffect(() => {
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup',   onMouseUp);
    return () => {
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup',   onMouseUp);
    };
  }, [onMouseMove, onMouseUp]);

  // Node drag — starts pending, activates only after threshold
  const startNodeDrag = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    didDragRef.current = false;
    const pos = positions[id] ?? { x: 0, y: 0 };
    const sc  = scaleRef.current;
    const { x: px, y: py } = panRef.current;
    pendingRef.current = {
      id,
      ox: (e.clientX - px) / sc - pos.x,
      oy: (e.clientY - py) / sc - pos.y,
      sx: e.clientX,
      sy: e.clientY,
    };
  };

  // Click registered only when no drag happened
  const handleNodeClick = (e: React.MouseEvent, model: Model) => {
    e.stopPropagation();
    if (!didDragRef.current) {
      setSelected((prev) => (prev?.name === model.name ? null : model));
    }
  };

  // Canvas pan
  const startPan = (e: React.MouseEvent) => {
    panningRef.current = { mx: e.clientX, my: e.clientY, px: pan.x, py: pan.y };
  };

  // Zoom toward cursor
  const handleWheel = useCallback((e: WheelEvent) => {
    e.preventDefault();
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    setScale((prev) => {
      const next  = Math.min(2, Math.max(0.25, prev - e.deltaY * 0.0008));
      const ratio = next / prev;
      setPan((p) => ({ x: mx - ratio * (mx - p.x), y: my - ratio * (my - p.y) }));
      return next;
    });
  }, []);

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    el.addEventListener('wheel', handleWheel, { passive: false });
    return () => el.removeEventListener('wheel', handleWheel);
  }, [handleWheel]);

  const edges = getEdges(models);

  // ── Render ───────────────────────────────────────────────────────────────────

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>

      {/* ── Toolbar: zoom + code toggle only ── */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 6, padding: '5px 10px', flexShrink: 0,
        background: 'var(--vscode-editorWidget-background, #252526)',
        borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
      }}>
        <div style={{ flex: 1 }} />
        <button style={btnBase} onClick={() => setScale((s) => Math.max(0.25, +(s - 0.1).toFixed(2)))}>−</button>
        <span style={{ fontSize: 11, opacity: 0.55, minWidth: 38, textAlign: 'center' }}>
          {Math.round(scale * 100)}%
        </span>
        <button style={btnBase} onClick={() => setScale((s) => Math.min(2, +(s + 0.1).toFixed(2)))}>+</button>
        <button style={{ ...btnBase, marginLeft: 2 }} onClick={() => { setScale(1); setPan({ x: 32, y: 32 }); }}>↺</button>
        <div style={{ width: 1, height: 14, background: 'rgba(255,255,255,0.12)', margin: '0 4px' }} />
        <button
          style={{
            ...btnBase,
            borderColor: showCode ? 'var(--vscode-focusBorder,#007acc)' : 'rgba(255,255,255,0.15)',
            background:  showCode ? 'rgba(0,122,204,0.15)' : 'transparent',
          }}
          onClick={() => setShowCode((v) => !v)}
        >{'</>'} Code</button>
      </div>

      {/* ── Canvas + Detail panel ── */}
      <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>

        {/* Canvas */}
        <div
          ref={canvasRef}
          onMouseDown={startPan}
          onClick={() => setSelected(null)}
          style={{ flex: 1, position: 'relative', overflow: 'hidden', cursor: 'grab', background: 'var(--vscode-editor-background,#1e1e1e)' }}
        >
          {/* Dot grid — moves with pan */}
          <div style={{
            position: 'absolute', inset: 0, pointerEvents: 'none',
            backgroundImage: 'radial-gradient(circle, rgba(255,255,255,0.07) 1px, transparent 1px)',
            backgroundSize: `${24 * scale}px ${24 * scale}px`,
            backgroundPosition: `${pan.x % (24 * scale)}px ${pan.y % (24 * scale)}px`,
          }} />

          {/* Transform wrapper — pan + scale applied here */}
          <div style={{
            position: 'absolute', transformOrigin: '0 0',
            transform: `translate(${pan.x}px,${pan.y}px) scale(${scale})`,
          }}>
            {/* SVG edges */}
            <svg style={{ position: 'absolute', overflow: 'visible', width: 0, height: 0, pointerEvents: 'none' }}>
              <defs>
                <marker id="vt-arrow" markerWidth="7" markerHeight="7" refX="5" refY="3" orient="auto">
                  <path d="M0,0.5 L0,5.5 L6,3 z" fill="rgba(99,102,241,0.65)" />
                </marker>
              </defs>
              {edges.map((edge, i) => {
                const from = positions[edge.from];
                const to   = positions[edge.to];
                if (!from || !to) return null;
                const x1 = from.x + NODE_W;
                const y1 = from.y + HEADER_H / 2;
                const x2 = to.x;
                const y2 = to.y + HEADER_H / 2;
                const cx = (x1 + x2) / 2;
                return (
                  <path key={i}
                    d={`M${x1} ${y1} C${cx} ${y1},${cx} ${y2},${x2} ${y2}`}
                    fill="none" stroke="rgba(99,102,241,0.45)" strokeWidth="1.5"
                    strokeDasharray="5 3" markerEnd="url(#vt-arrow)"
                  />
                );
              })}
            </svg>

            {/* Nodes */}
            {models.map((model) => {
              const pos      = positions[model.name] ?? { x: 0, y: 0 };
              const color    = modelColor(model.name);
              const isSel    = selected?.name === model.name;
              return (
                <div key={model.name}
                  onMouseDown={(e) => startNodeDrag(e, model.name)}
                  onClick={(e) => handleNodeClick(e, model)}
                  style={{
                    position: 'absolute', left: pos.x, top: pos.y,
                    width: NODE_W, height: nodeHeight(model),
                    borderRadius: 8, overflow: 'hidden', userSelect: 'none', cursor: 'pointer',
                    border: `1.5px solid ${isSel ? color : 'rgba(255,255,255,0.1)'}`,
                    boxShadow: isSel
                      ? `0 0 0 3px ${color}28, 0 4px 18px rgba(0,0,0,0.45)`
                      : '0 2px 10px rgba(0,0,0,0.35)',
                    background: 'var(--vscode-editor-background,#1e1e1e)',
                    transition: 'border-color 0.12s, box-shadow 0.12s',
                  }}
                >
                  <div style={{
                    height: HEADER_H, display: 'flex', alignItems: 'center',
                    padding: '0 12px', gap: 8,
                    background: `${color}18`, borderBottom: `1px solid ${color}28`,
                  }}>
                    <div style={{ width: 9, height: 9, borderRadius: '50%', background: color, flexShrink: 0 }} />
                    <span style={{ fontWeight: 700, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {model.name}
                    </span>
                  </div>
                  {model.fields.map((f, fi) => (
                    <div key={f.name} style={{
                      height: FIELD_H, display: 'flex', alignItems: 'center',
                      padding: '0 12px', gap: 6,
                      borderBottom: fi < model.fields.length - 1 ? '1px solid rgba(255,255,255,0.04)' : 'none',
                    }}>
                      <span style={{ fontSize: 9, width: 10, textAlign: 'center', flexShrink: 0, opacity: 0.5 }}>
                        {f.isPrimary ? '⬡' : f.isRelation ? '⇢' : '·'}
                      </span>
                      <span style={{ fontSize: 11, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {f.name}
                      </span>
                      <span style={{ fontSize: 10, flexShrink: 0, fontFamily: 'monospace', opacity: f.isRelation ? 0.8 : 0.4, color: f.isRelation ? color : 'inherit' }}>
                        {f.type}
                      </span>
                    </div>
                  ))}
                </div>
              );
            })}
          </div>

          {models.length === 0 && (
            <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', opacity: 0.3, pointerEvents: 'none' }}>
              <span style={{ fontSize: 12 }}>스키마를 파싱하는 중...</span>
            </div>
          )}
        </div>

        {/* ── Detail panel ── */}
        {selected && (
          <div style={{
            width: 276, flexShrink: 0,
            borderLeft: '1px solid var(--vscode-panel-border,rgba(255,255,255,0.1))',
            display: 'flex', flexDirection: 'column',
            background: 'var(--vscode-sideBar-background,#252526)',
            animation: 'slideInRight 0.15s ease-out',
          }}>
            {/* Header */}
            <div style={{
              display: 'flex', alignItems: 'center', justifyContent: 'space-between',
              padding: '10px 14px', flexShrink: 0,
              borderBottom: '1px solid var(--vscode-panel-border,rgba(255,255,255,0.1))',
            }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
                <div style={{ width: 10, height: 10, borderRadius: '50%', background: modelColor(selected.name), flexShrink: 0 }} />
                <span style={{ fontWeight: 700, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{selected.name}</span>
                {/* ORM type badge */}
                <span style={{
                  fontSize: 10, padding: '1px 7px', borderRadius: 10, flexShrink: 0,
                  background: 'rgba(99,102,241,0.12)', color: '#a5b4fc',
                  border: '1px solid rgba(99,102,241,0.22)',
                }}>{state.ormType}</span>
              </div>
              <button
                onClick={(e) => { e.stopPropagation(); setSelected(null); }}
                style={{ background: 'none', border: 'none', color: 'var(--vscode-foreground)', fontSize: 14, cursor: 'pointer', opacity: 0.45, padding: 2, lineHeight: 1 }}
              >✕</button>
            </div>

            {/* Fields list */}
            <div style={{ padding: '10px 14px', flexShrink: 0, borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
              {selected.fields.map((f) => (
                <div key={f.name} style={{ display: 'flex', alignItems: 'baseline', gap: 6, marginBottom: 5 }}>
                  <span style={{ fontSize: 9, width: 12, textAlign: 'center', opacity: 0.4, flexShrink: 0 }}>
                    {f.isPrimary ? '⬡' : f.isRelation ? '⇢' : '·'}
                  </span>
                  <span style={{ fontSize: 12, flex: 1 }}>{f.name}</span>
                  <span style={{ fontSize: 10, fontFamily: 'monospace', flexShrink: 0, opacity: f.isRelation ? 0.75 : 0.4, color: f.isRelation ? modelColor(f.type.replace('[]','').replace('?','')) : 'inherit' }}>
                    {f.type}
                  </span>
                </div>
              ))}
            </div>

            {/* Export JSON */}
            <div style={{ padding: '8px 14px 4px', fontSize: 10, opacity: 0.38, flexShrink: 0, letterSpacing: '0.06em' }}>
              EXPORT JSON
            </div>
            <pre style={{
              flex: 1, margin: 0, padding: '0 14px 14px', overflow: 'auto',
              fontFamily: 'var(--vscode-editor-font-family,Consolas,monospace)',
              fontSize: 11, lineHeight: 1.65,
              color: 'var(--vscode-editor-foreground,#d4d4d4)',
              background: 'transparent', whiteSpace: 'pre',
            }}>
              {JSON.stringify(modelToJson(selected, state.ormType), null, 2)}
            </pre>
          </div>
        )}
      </div>

      {/* ── Code drawer ── */}
      {showCode && (
        <div style={{
          height: 220, flexShrink: 0,
          borderTop: '1px solid var(--vscode-panel-border,rgba(255,255,255,0.1))',
          display: 'flex', flexDirection: 'column',
          animation: 'slideInUp 0.15s ease-out',
        }}>
          {/* ORM selector lives here */}
          <div style={{
            display: 'flex', gap: 4, padding: '5px 10px', flexShrink: 0, alignItems: 'center',
            borderBottom: '1px solid rgba(255,255,255,0.05)',
          }}>
            {ORM_TYPES.map((orm) => (
              <button key={orm}
                onClick={() => setState((p) => ({ ...p, ormType: orm, ormSource: DEFAULT_SCHEMAS[orm] }))}
                style={{
                  padding: '2px 8px', border: '1px solid', borderRadius: 3, fontSize: 10, cursor: 'pointer',
                  borderColor: state.ormType === orm ? 'var(--vscode-focusBorder,#007acc)' : 'rgba(255,255,255,0.15)',
                  background:  state.ormType === orm ? 'var(--vscode-button-background,#0e639c)' : 'transparent',
                  color:       state.ormType === orm ? 'var(--vscode-button-foreground,#fff)' : 'var(--vscode-foreground)',
                }}
              >{orm}</button>
            ))}
          </div>
          <textarea
            value={state.ormSource}
            onChange={(e) => setState((p) => ({ ...p, ormSource: e.target.value }))}
            spellCheck={false}
            style={{
              flex: 1, resize: 'none', border: 'none', outline: 'none',
              padding: '8px 12px',
              fontFamily: 'var(--vscode-editor-font-family,Consolas,monospace)',
              fontSize: 12, color: 'var(--vscode-editor-foreground,#d4d4d4)',
              background: 'var(--vscode-editor-background,#1e1e1e)',
              lineHeight: 1.6,
            }}
          />
        </div>
      )}
    </div>
  );
}
