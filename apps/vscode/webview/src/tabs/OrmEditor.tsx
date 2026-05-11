import React, { useState, useEffect, useRef, useCallback } from 'react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';
import { DEFAULT_SCHEMAS } from '../App';

// ── Types ─────────────────────────────────────────────────────────────────────

type Field = { name: string; type: string; isPrimary: boolean; isRelation: boolean };
type Model = { name: string; fields: Field[] };
type Pos   = { x: number; y: number };
type Layout = 'grid' | 'lr' | 'tb';

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

// ── Prisma serializer ─────────────────────────────────────────────────────────

function modelsToPrisma(models: Model[]): string {
  return models.map(model => {
    const fields = model.fields.map(f => {
      const parts: string[] = ['  ' + f.name.padEnd(14), f.type];
      if (f.isPrimary) parts.push('@id @default(autoincrement())');
      return parts.join('  ').trimEnd();
    });
    return `model ${model.name} {\n${fields.join('\n')}\n}`;
  }).join('\n\n');
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

type EdgeDef = { from: string; fromFieldIdx: number; to: string };

function getEdges(models: Model[]): EdgeDef[] {
  const names = new Set(models.map((m) => m.name));
  const edges: EdgeDef[] = [];
  for (const model of models) {
    for (let fi = 0; fi < model.fields.length; fi++) {
      const f = model.fields[fi];
      if (!f.isRelation) continue;
      const base = f.type.replace('[]', '').replace('?', '');
      if (!names.has(base) || base === model.name) continue;
      // Only draw from FK-owning side (has a matching scalar like authorId / author_id)
      const hasFk = model.fields.some(
        (sf) => !sf.isRelation && (
          sf.name === f.name + 'Id' || sf.name === f.name + '_id'
        )
      );
      if (!hasFk) continue;
      edges.push({ from: model.name, fromFieldIdx: fi, to: base });
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

// ── Layout ────────────────────────────────────────────────────────────────────

function computeLayout(models: Model[], edges: EdgeDef[], layout: Layout): Record<string, Pos> {
  if (layout === 'grid') {
    const cols = Math.max(1, Math.ceil(Math.sqrt(models.length * 1.5)));
    const result: Record<string, Pos> = {};
    models.forEach((m, i) => {
      result[m.name] = { x: 40 + (i % cols) * (NODE_W + 60), y: 40 + Math.floor(i / cols) * 280 };
    });
    return result;
  }
  // BFS level assignment
  const childrenOf = new Map<string, string[]>();
  const inDegree   = new Map<string, number>();
  for (const m of models) { childrenOf.set(m.name, []); inDegree.set(m.name, 0); }
  for (const e of edges)  { childrenOf.get(e.from)?.push(e.to); inDegree.set(e.to, (inDegree.get(e.to) ?? 0) + 1); }

  const level = new Map<string, number>();
  const queue: string[] = [];
  for (const m of models) if ((inDegree.get(m.name) ?? 0) === 0) { level.set(m.name, 0); queue.push(m.name); }
  for (let qi = 0; qi < queue.length; qi++) {
    const curr = queue[qi];
    for (const next of (childrenOf.get(curr) ?? [])) {
      const l = (level.get(curr) ?? 0) + 1;
      if (!level.has(next) || (level.get(next) ?? 0) < l) { level.set(next, l); queue.push(next); }
    }
  }
  const maxL = Math.max(0, ...level.values());
  for (const m of models) if (!level.has(m.name)) level.set(m.name, maxL + 1);

  const byLevel = new Map<number, string[]>();
  for (const [name, l] of level.entries()) { if (!byLevel.has(l)) byLevel.set(l, []); byLevel.get(l)!.push(name); }

  const result: Record<string, Pos> = {};
  for (const [l, names] of byLevel.entries()) {
    names.forEach((name, i) => {
      result[name] = layout === 'lr'
        ? { x: 40 + l * (NODE_W + 80), y: 40 + i * 280 }
        : { x: 40 + i * (NODE_W + 60), y: 40 + l * 280 };
    });
  }
  return result;
}

// ── Icons ─────────────────────────────────────────────────────────────────────

const IconFit = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
    <path d="M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M3 16v3a2 2 0 0 0 2 2h3M16 21h3a2 2 0 0 0 2-2v-3"/>
  </svg>
);

const IconHand = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M18 11V8a2 2 0 0 0-4 0m4 3v-1a2 2 0 0 1 4 0v5a8 8 0 0 1-8 8h-2a8 8 0 0 1-8-8v-1a2 2 0 0 1 4 0M14 8V6a2 2 0 0 0-4 0v5M10 7V5a2 2 0 0 0-4 0v9"/>
  </svg>
);

const IconSun = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
    <circle cx="12" cy="12" r="4"/>
    <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/>
  </svg>
);

const IconMoon = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
    <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>
  </svg>
);

const IconGrid = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
    <rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/>
    <rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>
  </svg>
);

const IconLR = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="1" y="8" width="6" height="8" rx="1"/><rect x="17" y="4" width="6" height="6" rx="1"/>
    <rect x="17" y="14" width="6" height="6" rx="1"/>
    <line x1="7" y1="12" x2="12" y2="12"/><line x1="12" y1="7" x2="12" y2="17"/>
    <line x1="12" y1="7"  x2="17" y2="7"/><line x1="12" y1="17" x2="17" y2="17"/>
  </svg>
);

const IconTB = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="8" y="1" width="8" height="6" rx="1"/><rect x="3" y="17" width="6" height="6" rx="1"/>
    <rect x="15" y="17" width="6" height="6" rx="1"/>
    <line x1="12" y1="7" x2="12" y2="12"/><line x1="6" y1="12" x2="18" y2="12"/>
    <line x1="6"  y1="12" x2="6"  y2="17"/><line x1="18" y1="12" x2="18" y2="17"/>
  </svg>
);

// ── Button style ──────────────────────────────────────────────────────────────

const btnBase: React.CSSProperties = {
  padding: '2px 8px', border: '1px solid rgba(255,255,255,0.15)', borderRadius: 3,
  background: 'transparent', color: 'var(--vscode-foreground)', fontSize: 11, cursor: 'pointer',
};

const navBtn = (active = false): React.CSSProperties => ({
  display: 'flex', alignItems: 'center', justifyContent: 'center',
  width: 28, height: 26, border: 'none', borderRadius: 5, cursor: 'pointer',
  background: active ? 'rgba(99,102,241,0.18)' : 'transparent',
  color: active ? '#a5b4fc' : 'var(--vscode-foreground)',
  opacity: active ? 1 : 0.7,
  transition: 'background 0.12s, color 0.12s, opacity 0.12s',
});

const navDivider: React.CSSProperties = {
  width: 1, height: 16, background: 'var(--vscode-panel-border, rgba(255,255,255,0.12))', margin: '0 3px', flexShrink: 0,
};

// ── Inline edit types ─────────────────────────────────────────────────────────

type InlineEdit = {
  modelName: string;
  aspect: 'modelName' | 'fieldName' | 'fieldType';
  fieldIdx?: number;
  value: string;
};

// ── Component ─────────────────────────────────────────────────────────────────

type Props = { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> };

export default function OrmEditor({ state, setState }: Props) {
  const [models,       setModels]       = useState<Model[]>([]);
  const [positions,    setPositions]    = useState<Record<string, Pos>>({});
  const [selectedIds,  setSelectedIds]  = useState<Set<string>>(new Set());
  const [selectedEdge, setSelectedEdge] = useState<EdgeDef | null>(null);
  const [inlineEdit,   setInlineEdit]   = useState<InlineEdit | null>(null);
  const [lasso,        setLasso]        = useState<{ sx: number; sy: number; ex: number; ey: number } | null>(null);
  const [layout,       setLayout]       = useState<Layout>('grid');
  const [showCode,     setShowCode]     = useState(false);
  const [lockMode,     setLockMode]     = useState(false);
  const [pan,   setPan]   = useState<Pos>({ x: 32, y: 32 });
  const [scale, setScale] = useState(1);

  // Keep refs in sync so event handlers never go stale
  const panRef   = useRef(pan);
  const scaleRef = useRef(scale);
  useEffect(() => { panRef.current = pan; },   [pan]);
  useEffect(() => { scaleRef.current = scale; }, [scale]);

  // Refs for mutable state in event handlers
  const selectedIdsRef = useRef(selectedIds);
  const modelsRef      = useRef(models);
  const posRef         = useRef(positions);
  const fromEditRef    = useRef(false);

  useEffect(() => { selectedIdsRef.current = selectedIds; }, [selectedIds]);
  useEffect(() => { modelsRef.current = models; }, [models]);
  useEffect(() => { posRef.current = positions; }, [positions]);

  // Drag / pan / lasso state
  const pendingRef  = useRef<{ id: string; sx: number; sy: number } | null>(null);
  const draggingRef = useRef<{
    id: string;
    startPositions: Record<string, Pos>;
    mouseStart: { x: number; y: number };
  } | null>(null);
  const didDragRef    = useRef(false);
  const panningRef    = useRef<{ mx: number; my: number; px: number; py: number } | null>(null);
  const lassoingRef   = useRef(false);
  const lassoRef      = useRef<{ sx: number; sy: number; ex: number; ey: number } | null>(null);

  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const canvasRef   = useRef<HTMLDivElement>(null);

  // ── Parse ORM source ────────────────────────────────────────────────────────

  useEffect(() => {
    if (fromEditRef.current) {
      fromEditRef.current = false;
      return;
    }
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

  // Keep selectedIds fresh when models re-parse
  useEffect(() => {
    const names = new Set(models.map(m => m.name));
    setSelectedIds(prev => {
      const next = new Set([...prev].filter(id => names.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [models]);

  // ── Mouse handlers ───────────────────────────────────────────────────────────

  const onMouseMove = useCallback((e: MouseEvent) => {
    const sc = scaleRef.current;

    // Promote pending → dragging once threshold exceeded
    if (pendingRef.current && !draggingRef.current) {
      const dx = Math.abs(e.clientX - pendingRef.current.sx);
      const dy = Math.abs(e.clientY - pendingRef.current.sy);
      if (dx > DRAG_THRESHOLD || dy > DRAG_THRESHOLD) {
        const id = pendingRef.current.id;
        const curSelected = selectedIdsRef.current;
        const curPos = posRef.current;
        const startPositions: Record<string, Pos> = {};
        if (curSelected.has(id)) {
          // Group drag: store all selected nodes' start positions
          for (const sid of curSelected) {
            startPositions[sid] = { ...(curPos[sid] ?? { x: 0, y: 0 }) };
          }
        } else {
          // Single drag
          startPositions[id] = { ...(curPos[id] ?? { x: 0, y: 0 }) };
        }
        draggingRef.current = {
          id,
          startPositions,
          mouseStart: { x: e.clientX, y: e.clientY },
        };
        didDragRef.current = true;
        pendingRef.current = null;
      }
    }

    if (draggingRef.current) {
      const { startPositions, mouseStart } = draggingRef.current;
      const ddx = (e.clientX - mouseStart.x) / sc;
      const ddy = (e.clientY - mouseStart.y) / sc;
      setPositions((prev) => {
        const next = { ...prev };
        for (const [sid, startPos] of Object.entries(startPositions)) {
          next[sid] = { x: startPos.x + ddx, y: startPos.y + ddy };
        }
        return next;
      });
    } else if (panningRef.current) {
      const { mx, my, px: ppx, py: ppy } = panningRef.current;
      setPan({ x: ppx + e.clientX - mx, y: ppy + e.clientY - my });
    } else if (lassoingRef.current && lassoRef.current) {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const cx = (e.clientX - rect.left - panRef.current.x) / sc;
      const cy = (e.clientY - rect.top  - panRef.current.y) / sc;
      lassoRef.current = { ...lassoRef.current, ex: cx, ey: cy };
      setLasso({ ...lassoRef.current });
    }
  }, []);

  const onMouseUp = useCallback(() => {
    // If lasso was active: compute selection
    if (lassoingRef.current && lassoRef.current) {
      const l = lassoRef.current;
      const lx1 = Math.min(l.sx, l.ex);
      const lx2 = Math.max(l.sx, l.ex);
      const ly1 = Math.min(l.sy, l.ey);
      const ly2 = Math.max(l.sy, l.ey);
      const dragDist = Math.abs(l.ex - l.sx) + Math.abs(l.ey - l.sy);
      if (dragDist >= 10) {
        const curPos = posRef.current;
        const curModels = modelsRef.current;
        const newIds = new Set<string>();
        for (const model of curModels) {
          const pos = curPos[model.name];
          if (!pos) continue;
          const nh = nodeHeight(model);
          // Check bounding box overlap
          if (pos.x < lx2 && pos.x + NODE_W > lx1 && pos.y < ly2 && pos.y + nh > ly1) {
            newIds.add(model.name);
          }
        }
        setSelectedIds(newIds);
      } else {
        // Tiny drag = deselect all
        setSelectedIds(new Set());
      }
      lassoingRef.current = false;
      lassoRef.current    = null;
      setLasso(null);
    }

    draggingRef.current = null;
    pendingRef.current  = null;
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
    if (lockMode) return;
    e.stopPropagation();
    didDragRef.current = false;
    pendingRef.current = { id, sx: e.clientX, sy: e.clientY };
  };

  // Click registered only when no drag happened
  const handleNodeClick = (e: React.MouseEvent, model: Model) => {
    e.stopPropagation();
    if (didDragRef.current) return;
    setSelectedEdge(null);
    setSelectedIds(new Set([model.name]));
  };

  // Canvas mousedown: pan in lockMode, lasso otherwise
  const startCanvasMouseDown = (e: React.MouseEvent) => {
    if (lockMode) {
      panningRef.current = { mx: e.clientX, my: e.clientY, px: pan.x, py: pan.y };
    } else {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const cx = (e.clientX - rect.left - pan.x) / scale;
      const cy = (e.clientY - rect.top  - pan.y) / scale;
      lassoRef.current    = { sx: cx, sy: cy, ex: cx, ey: cy };
      lassoingRef.current = true;
    }
  };

  // Zoom toward cursor (pinch) or pan (two-finger scroll)
  const handleWheel = useCallback((e: WheelEvent) => {
    e.preventDefault();
    if (e.ctrlKey || e.metaKey) {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      setScale((prev) => {
        const next  = Math.min(2, Math.max(0.25, prev - e.deltaY * 0.008));
        const ratio = next / prev;
        setPan((p) => ({ x: mx - ratio * (mx - p.x), y: my - ratio * (my - p.y) }));
        return next;
      });
    } else {
      setPan((p) => ({ x: p.x - e.deltaX, y: p.y - e.deltaY }));
    }
  }, []);

  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    el.addEventListener('wheel', handleWheel, { passive: false });
    return () => el.removeEventListener('wheel', handleWheel);
  }, [handleWheel]);

  const edges = getEdges(models);

  // ── Inline edit helpers ───────────────────────────────────────────────────────

  const commitInlineEdit = (value: string) => {
    if (!inlineEdit || state.ormType !== 'prisma') { setInlineEdit(null); return; }
    const { modelName, aspect, fieldIdx } = inlineEdit;
    const updated = models.map(m => {
      if (m.name !== modelName) return m;
      if (aspect === 'modelName') {
        return { ...m, name: value };
      }
      if (aspect === 'fieldName' && fieldIdx !== undefined) {
        const fields = m.fields.map((f, i) => i === fieldIdx ? { ...f, name: value } : f);
        return { ...m, fields };
      }
      if (aspect === 'fieldType' && fieldIdx !== undefined) {
        const base = value.replace('[]', '').replace('?', '');
        const isRel = !PRISMA_SCALARS.has(base) && /^[A-Z]/.test(base);
        const fields = m.fields.map((f, i) => i === fieldIdx ? { ...f, type: value, isRelation: isRel } : f);
        return { ...m, fields };
      }
      return m;
    });
    const newSource = modelsToPrisma(updated);
    setModels(updated);
    // Update positions if model was renamed
    if (aspect === 'modelName' && value !== modelName) {
      setPositions(prev => {
        const next = { ...prev };
        if (next[modelName]) { next[value] = next[modelName]; delete next[modelName]; }
        return next;
      });
      setSelectedIds(prev => {
        if (prev.has(modelName)) {
          const next = new Set(prev);
          next.delete(modelName);
          next.add(value);
          return next;
        }
        return prev;
      });
    }
    fromEditRef.current = true;
    setState(p => ({ ...p, ormSource: newSource }));
    setInlineEdit(null);
  };

  const handleAddField = (modelName: string) => {
    if (state.ormType !== 'prisma') return;
    const updated = models.map(m => {
      if (m.name !== modelName) return m;
      const newField: Field = { name: 'newField', type: 'String', isPrimary: false, isRelation: false };
      return { ...m, fields: [...m.fields, newField] };
    });
    const newSource = modelsToPrisma(updated);
    setModels(updated);
    fromEditRef.current = true;
    setState(p => ({ ...p, ormSource: newSource }));
  };

  const handleDeleteField = (modelName: string, fieldIdx: number) => {
    if (state.ormType !== 'prisma') return;
    const updated = models.map(m => {
      if (m.name !== modelName) return m;
      const fields = m.fields.filter((_, i) => i !== fieldIdx);
      return { ...m, fields };
    });
    const newSource = modelsToPrisma(updated);
    setModels(updated);
    fromEditRef.current = true;
    setState(p => ({ ...p, ormSource: newSource }));
  };

  // ── Edge actions ──────────────────────────────────────────────────────────────

  const handleDeleteEdge = (edge: EdgeDef) => {
    if (state.ormType !== 'prisma') return;
    const relField = models.find(m => m.name === edge.from)?.fields[edge.fromFieldIdx];
    if (!relField) return;
    const updated = models.map(m => {
      if (m.name !== edge.from) return m;
      // Remove the relation field and its matching FK scalar
      const fields = m.fields.filter((f, i) => {
        if (i === edge.fromFieldIdx) return false;
        if (!f.isRelation && (f.name === relField.name + 'Id' || f.name === relField.name + '_id')) return false;
        return true;
      });
      return { ...m, fields };
    });
    const newSource = modelsToPrisma(updated);
    setModels(updated);
    setSelectedEdge(null);
    fromEditRef.current = true;
    setState(p => ({ ...p, ormSource: newSource }));
  };

  const handleToggleEdgeRelType = (edge: EdgeDef) => {
    if (state.ormType !== 'prisma') return;
    const updated = models.map(m => {
      if (m.name !== edge.from) return m;
      const fields = m.fields.map((f, i) => {
        if (i !== edge.fromFieldIdx) return f;
        const base = f.type.replace('[]', '').replace('?', '');
        const newType = f.type.endsWith('[]') ? base : base + '[]';
        const isRel = !PRISMA_SCALARS.has(base) && /^[A-Z]/.test(base);
        return { ...f, type: newType, isRelation: isRel };
      });
      return { ...m, fields };
    });
    const newSource = modelsToPrisma(updated);
    setModels(updated);
    // Update selectedEdge to reflect new field index (it doesn't change)
    setSelectedEdge({ ...edge });
    fromEditRef.current = true;
    setState(p => ({ ...p, ormSource: newSource }));
  };

  // ── Layout application ────────────────────────────────────────────────────────

  const applyLayout = (l: Layout) => {
    setLayout(l);
    const newPos = computeLayout(models, edges, l);
    setPositions(newPos);
    setPan({ x: 32, y: 32 });
    setScale(1);
  };

  // ── Derived panel state ───────────────────────────────────────────────────────

  const singleSelected = selectedIds.size === 1
    ? models.find(m => m.name === [...selectedIds][0]) ?? null
    : null;
  const showRightPanel = singleSelected !== null || selectedEdge !== null || selectedIds.size > 1;

  // ── Edge field name helper ────────────────────────────────────────────────────
  const getEdgeRelFieldName = (edge: EdgeDef): string => {
    const model = models.find(m => m.name === edge.from);
    return model?.fields[edge.fromFieldIdx]?.name ?? '';
  };

  const getEdgeRelType = (edge: EdgeDef): string => {
    const model = models.find(m => m.name === edge.from);
    const f = model?.fields[edge.fromFieldIdx];
    if (!f) return 'many-to-one';
    return f.type.endsWith('[]') ? 'one-to-many' : 'many-to-one';
  };

  // ── Render ───────────────────────────────────────────────────────────────────

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>

      {/* ── Toolbar: Code toggle only ── */}
      <div style={{
        display: 'flex', alignItems: 'center', padding: '5px 10px', flexShrink: 0,
        background: 'var(--vscode-editorWidget-background, #252526)',
        borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
      }}>
        <div style={{ flex: 1 }} />
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
          onMouseDown={startCanvasMouseDown}
          onClick={() => {
            if (!lassoingRef.current && !draggingRef.current) {
              setSelectedIds(new Set());
              setSelectedEdge(null);
            }
          }}
          style={{ flex: 1, position: 'relative', overflow: 'hidden', cursor: lockMode ? 'grab' : 'default', background: 'var(--vscode-editor-background,#1e1e1e)' }}
        >
          {/* Dot grid — moves with pan */}
          <div style={{
            position: 'absolute', inset: 0, pointerEvents: 'none',
            backgroundImage: 'radial-gradient(circle, var(--canvas-dot) 1px, transparent 1px)',
            backgroundSize: `${24 * scale}px ${24 * scale}px`,
            backgroundPosition: `${pan.x % (24 * scale)}px ${pan.y % (24 * scale)}px`,
          }} />

          {/* Transform wrapper — pan + scale applied here */}
          <div style={{
            position: 'absolute', transformOrigin: '0 0',
            transform: `translate(${pan.x}px,${pan.y}px) scale(${scale})`,
          }}>
            {/* SVG edges — use style={} not presentation attrs so CSS vars work */}
            <svg style={{ position: 'absolute', overflow: 'visible', width: 0, height: 0 }}>
              <defs>
                <marker id="vt-arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
                  <path d="M0,0.5 L0,5.5 L6.5,3 z" style={{ fill: 'var(--edge-arrow, rgba(99,102,241,0.8))' }} />
                </marker>
                <marker id="vt-arrow-sel" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
                  <path d="M0,0.5 L0,5.5 L6.5,3 z" style={{ fill: 'rgba(129,140,248,1)' }} />
                </marker>
              </defs>
              {edges.map((edge, i) => {
                const from = positions[edge.from];
                const to   = positions[edge.to];
                if (!from || !to) return null;
                const fromModel = models.find(m => m.name === edge.from);
                const toModel   = models.find(m => m.name === edge.to);
                if (!fromModel || !toModel) return null;
                const y1 = from.y + HEADER_H + edge.fromFieldIdx * FIELD_H + FIELD_H / 2;
                const y2 = to.y + HEADER_H / 2;
                const fromRight = from.x + NODE_W;
                const toLeft    = to.x;
                const fromLeft  = from.x;
                const toRight   = to.x + NODE_W;
                const useRight  = Math.abs(fromRight - toLeft) <= Math.abs(fromLeft - toRight);
                const x1 = useRight ? fromRight : fromLeft;
                const x2 = useRight ? toLeft    : toRight;
                const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
                const c1x = x1 + (useRight ?  dx : -dx);
                const c2x = x2 + (useRight ? -dx :  dx);
                const pathD = `M${x1} ${y1} C${c1x} ${y1},${c2x} ${y2},${x2} ${y2}`;
                const isSelEdge = selectedEdge?.from === edge.from &&
                                  selectedEdge?.to === edge.to &&
                                  selectedEdge?.fromFieldIdx === edge.fromFieldIdx;
                return (
                  <g key={i}>
                    {/* Visible path */}
                    <path
                      d={pathD}
                      fill="none"
                      style={{
                        stroke: isSelEdge ? 'rgba(129,140,248,1)' : 'var(--edge-color, rgba(99,102,241,0.55))',
                        strokeWidth: isSelEdge ? 2 : 1.5,
                      }}
                      markerEnd={isSelEdge ? 'url(#vt-arrow-sel)' : 'url(#vt-arrow)'}
                    />
                    <circle cx={x1} cy={y1} r={3} style={{ fill: isSelEdge ? 'rgba(129,140,248,1)' : 'var(--edge-color, rgba(99,102,241,0.55))' }} />
                    {/* Transparent hit target */}
                    <path
                      d={pathD}
                      fill="none"
                      stroke="transparent"
                      strokeWidth={12}
                      style={{ cursor: 'pointer' }}
                      onClick={(e) => {
                        e.stopPropagation();
                        setSelectedEdge(edge);
                        setSelectedIds(new Set());
                      }}
                    />
                  </g>
                );
              })}
            </svg>

            {/* Lasso rectangle */}
            {lasso && (
              <div style={{
                position: 'absolute', pointerEvents: 'none',
                left: Math.min(lasso.sx, lasso.ex), top: Math.min(lasso.sy, lasso.ey),
                width: Math.abs(lasso.ex - lasso.sx), height: Math.abs(lasso.ey - lasso.sy),
                border: '1px dashed rgba(99,102,241,0.7)',
                background: 'rgba(99,102,241,0.07)',
              }} />
            )}

            {/* Nodes */}
            {models.map((model) => {
              const pos   = positions[model.name] ?? { x: 0, y: 0 };
              const color = modelColor(model.name);
              const isSel = selectedIds.has(model.name);
              return (
                <div key={model.name}
                  onMouseDown={(e) => startNodeDrag(e, model.name)}
                  onClick={(e) => handleNodeClick(e, model)}
                  style={{
                    position: 'absolute', left: pos.x, top: pos.y,
                    width: NODE_W,
                    borderRadius: 8, overflow: 'hidden', userSelect: 'none', cursor: 'pointer',
                    border: `1.5px solid ${isSel ? color : 'var(--node-border)'}`,
                    boxShadow: isSel
                      ? `0 0 0 3px ${color}28, 0 4px 18px rgba(0,0,0,0.35)`
                      : 'var(--node-shadow)',
                    background: 'var(--node-bg)',
                    transition: 'border-color 0.12s, box-shadow 0.12s',
                  }}
                >
                  {/* Header */}
                  <div
                    style={{
                      height: HEADER_H, display: 'flex', alignItems: 'center',
                      padding: '0 12px', gap: 8,
                      background: `${color}18`, borderBottom: `1px solid ${color}28`,
                    }}
                    onDoubleClick={(e) => {
                      if (state.ormType !== 'prisma') return;
                      e.stopPropagation();
                      setInlineEdit({ modelName: model.name, aspect: 'modelName', value: model.name });
                    }}
                  >
                    <div style={{ width: 9, height: 9, borderRadius: '50%', background: color, flexShrink: 0 }} />
                    {inlineEdit?.modelName === model.name && inlineEdit.aspect === 'modelName' ? (
                      <input
                        autoFocus
                        value={inlineEdit.value}
                        onClick={(e) => e.stopPropagation()}
                        onChange={(e) => setInlineEdit(prev => prev ? { ...prev, value: e.target.value } : null)}
                        onBlur={(e) => commitInlineEdit(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') commitInlineEdit((e.target as HTMLInputElement).value);
                          if (e.key === 'Escape') setInlineEdit(null);
                          e.stopPropagation();
                        }}
                        style={{
                          fontWeight: 700, fontSize: 12, flex: 1,
                          background: 'rgba(0,0,0,0.3)', border: '1px solid rgba(99,102,241,0.5)',
                          borderRadius: 3, color: 'var(--vscode-foreground)', padding: '1px 4px', outline: 'none',
                        }}
                      />
                    ) : (
                      <span style={{ fontWeight: 700, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {model.name}
                      </span>
                    )}
                  </div>

                  {/* Fields */}
                  {model.fields.map((f, fi) => (
                    <div key={fi} style={{
                      height: FIELD_H, display: 'flex', alignItems: 'center',
                      padding: '0 12px', gap: 6,
                      borderBottom: fi < model.fields.length - 1 ? '1px solid var(--node-field-divider)' : 'none',
                    }}>
                      <span style={{ fontSize: 9, width: 10, textAlign: 'center', flexShrink: 0, opacity: 0.5 }}>
                        {f.isPrimary ? '⬡' : f.isRelation ? '⇢' : '·'}
                      </span>
                      {/* Field name */}
                      {inlineEdit?.modelName === model.name && inlineEdit.aspect === 'fieldName' && inlineEdit.fieldIdx === fi ? (
                        <input
                          autoFocus
                          value={inlineEdit.value}
                          onClick={(e) => e.stopPropagation()}
                          onChange={(e) => setInlineEdit(prev => prev ? { ...prev, value: e.target.value } : null)}
                          onBlur={(e) => commitInlineEdit(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') commitInlineEdit((e.target as HTMLInputElement).value);
                            if (e.key === 'Escape') setInlineEdit(null);
                            e.stopPropagation();
                          }}
                          style={{
                            fontSize: 11, flex: 1,
                            background: 'rgba(0,0,0,0.3)', border: '1px solid rgba(99,102,241,0.5)',
                            borderRadius: 3, color: 'var(--vscode-foreground)', padding: '0 3px', outline: 'none',
                          }}
                        />
                      ) : (
                        <span
                          style={{ fontSize: 11, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                          onDoubleClick={(e) => {
                            if (state.ormType !== 'prisma') return;
                            e.stopPropagation();
                            setInlineEdit({ modelName: model.name, aspect: 'fieldName', fieldIdx: fi, value: f.name });
                          }}
                        >
                          {f.name}
                        </span>
                      )}
                      {/* Field type */}
                      {inlineEdit?.modelName === model.name && inlineEdit.aspect === 'fieldType' && inlineEdit.fieldIdx === fi ? (
                        <input
                          autoFocus
                          value={inlineEdit.value}
                          onClick={(e) => e.stopPropagation()}
                          onChange={(e) => setInlineEdit(prev => prev ? { ...prev, value: e.target.value } : null)}
                          onBlur={(e) => commitInlineEdit(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') commitInlineEdit((e.target as HTMLInputElement).value);
                            if (e.key === 'Escape') setInlineEdit(null);
                            e.stopPropagation();
                          }}
                          style={{
                            fontSize: 10, fontFamily: 'monospace', width: 70,
                            background: 'rgba(0,0,0,0.3)', border: '1px solid rgba(99,102,241,0.5)',
                            borderRadius: 3, color: f.isRelation ? color : 'var(--vscode-foreground)', padding: '0 3px', outline: 'none',
                          }}
                        />
                      ) : (
                        <span
                          style={{ fontSize: 10, flexShrink: 0, fontFamily: 'monospace', opacity: f.isRelation ? 0.8 : 0.4, color: f.isRelation ? color : 'inherit' }}
                          onDoubleClick={(e) => {
                            if (state.ormType !== 'prisma') return;
                            e.stopPropagation();
                            setInlineEdit({ modelName: model.name, aspect: 'fieldType', fieldIdx: fi, value: f.type });
                          }}
                        >
                          {f.type}
                        </span>
                      )}
                    </div>
                  ))}

                  {/* Add Field button (Prisma only) */}
                  {state.ormType === 'prisma' && (
                    <div style={{ padding: '3px 8px', borderTop: '1px solid var(--node-field-divider)' }}>
                      <button
                        onClick={(e) => { e.stopPropagation(); handleAddField(model.name); }}
                        style={{
                          width: '100%', background: 'none', border: 'none', cursor: 'pointer',
                          fontSize: 10, color: 'var(--vscode-foreground)', opacity: 0.4,
                          padding: '2px 0', textAlign: 'center',
                        }}
                      >+ Add Field</button>
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          {models.length === 0 && (
            <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', opacity: 0.3, pointerEvents: 'none' }}>
              <span style={{ fontSize: 12 }}>스키마를 파싱하는 중...</span>
            </div>
          )}

          {/* ── Bottom navigation bar ── */}
          <div style={{
            position: 'absolute', bottom: 14, left: '50%', transform: 'translateX(-50%)',
            display: 'flex', alignItems: 'center', gap: 2,
            padding: '4px 8px',
            background: 'var(--navbar-bg)',
            border: '1px solid var(--navbar-border)',
            borderRadius: 9,
            boxShadow: '0 4px 16px rgba(0,0,0,0.25)',
            backdropFilter: 'blur(8px)',
            userSelect: 'none',
            zIndex: 10,
          }}>
            {/* Zoom − */}
            <button
              style={navBtn()}
              title="축소 (10%)"
              onClick={() => setScale((s) => Math.max(0.25, +(s - 0.1).toFixed(2)))}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
                <line x1="5" y1="12" x2="19" y2="12"/>
              </svg>
            </button>

            {/* Zoom % — click to reset to 100% */}
            <button
              style={{ ...navBtn(), width: 44, fontSize: 11, fontVariantNumeric: 'tabular-nums' }}
              title="100%로 초기화"
              onClick={() => { setScale(1); setPan({ x: 32, y: 32 }); }}
            >
              {Math.round(scale * 100)}%
            </button>

            {/* Zoom + */}
            <button
              style={navBtn()}
              title="확대 (10%)"
              onClick={() => setScale((s) => Math.min(2, +(s + 0.1).toFixed(2)))}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
                <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
              </svg>
            </button>

            <div style={navDivider} />

            {/* Fit to screen */}
            <button
              style={navBtn()}
              title="화면에 맞추기"
              onClick={() => { setScale(1); setPan({ x: 32, y: 32 }); }}
            >
              <IconFit />
            </button>

            <div style={navDivider} />

            {/* Pan-only mode */}
            <button
              style={navBtn(lockMode)}
              title={lockMode ? '이동 모드 (클릭하여 편집 모드로)' : '편집 모드 (클릭하여 이동 모드로)'}
              onClick={() => { setLockMode((v) => !v); if (lockMode) setSelectedIds(new Set()); }}
            >
              <IconHand />
            </button>

            <div style={navDivider} />

            {/* Theme toggle */}
            <button
              style={navBtn()}
              title={state.theme === 'dark' ? '라이트 모드로 전환' : '다크 모드로 전환'}
              onClick={() => setState((p) => ({ ...p, theme: p.theme === 'dark' ? 'light' : 'dark' }))}
            >
              {state.theme === 'dark' ? <IconSun /> : <IconMoon />}
            </button>

            <div style={navDivider} />

            {/* Layout: Grid */}
            <button
              style={navBtn(layout === 'grid')}
              title="격자 레이아웃"
              onClick={() => applyLayout('grid')}
            >
              <IconGrid />
            </button>

            {/* Layout: LR */}
            <button
              style={navBtn(layout === 'lr')}
              title="좌→우 트리 레이아웃"
              onClick={() => applyLayout('lr')}
            >
              <IconLR />
            </button>

            {/* Layout: TB */}
            <button
              style={navBtn(layout === 'tb')}
              title="위→아래 트리 레이아웃"
              onClick={() => applyLayout('tb')}
            >
              <IconTB />
            </button>
          </div>
        </div>

        {/* ── Right panel ── */}
        {showRightPanel && (
          <div style={{
            width: 276, flexShrink: 0,
            borderLeft: '1px solid var(--vscode-panel-border,rgba(255,255,255,0.1))',
            display: 'flex', flexDirection: 'column',
            background: 'var(--vscode-sideBar-background,#252526)',
            animation: 'slideInRight 0.15s ease-out',
          }}>
            {/* Multi-select panel */}
            {selectedIds.size > 1 && singleSelected === null && selectedEdge === null && (
              <div style={{ padding: '16px 14px', opacity: 0.6, fontSize: 12 }}>
                {selectedIds.size} nodes selected
              </div>
            )}

            {/* Node detail panel */}
            {singleSelected !== null && (
              <>
                {/* Header */}
                <div style={{
                  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                  padding: '10px 14px', flexShrink: 0,
                  borderBottom: '1px solid var(--vscode-panel-border,rgba(255,255,255,0.1))',
                }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
                    <div style={{ width: 10, height: 10, borderRadius: '50%', background: modelColor(singleSelected.name), flexShrink: 0 }} />
                    {inlineEdit?.modelName === singleSelected.name && inlineEdit.aspect === 'modelName' && (
                      // When editing model name inline on the canvas node, we don't render inline here
                      <span style={{ fontWeight: 700, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{singleSelected.name}</span>
                    )}
                    {!(inlineEdit?.modelName === singleSelected.name && inlineEdit.aspect === 'modelName') && (
                      <span style={{ fontWeight: 700, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{singleSelected.name}</span>
                    )}
                    {/* ORM type badge */}
                    <span style={{
                      fontSize: 10, padding: '1px 7px', borderRadius: 10, flexShrink: 0,
                      background: 'rgba(99,102,241,0.12)', color: '#a5b4fc',
                      border: '1px solid rgba(99,102,241,0.22)',
                    }}>{state.ormType}</span>
                  </div>
                  <button
                    onClick={(e) => { e.stopPropagation(); setSelectedIds(new Set()); }}
                    style={{ background: 'none', border: 'none', color: 'var(--vscode-foreground)', fontSize: 14, cursor: 'pointer', opacity: 0.45, padding: 2, lineHeight: 1 }}
                  >✕</button>
                </div>

                {/* Fields list */}
                <div style={{ padding: '10px 14px', flexShrink: 0, borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
                  {singleSelected.fields.map((f, fi) => (
                    <div key={fi} style={{ display: 'flex', alignItems: 'baseline', gap: 6, marginBottom: 5 }}>
                      <span style={{ fontSize: 9, width: 12, textAlign: 'center', opacity: 0.4, flexShrink: 0 }}>
                        {f.isPrimary ? '⬡' : f.isRelation ? '⇢' : '·'}
                      </span>
                      <span style={{ fontSize: 12, flex: 1 }}>{f.name}</span>
                      <span style={{ fontSize: 10, fontFamily: 'monospace', flexShrink: 0, opacity: f.isRelation ? 0.75 : 0.4, color: f.isRelation ? modelColor(f.type.replace('[]','').replace('?','')) : 'inherit' }}>
                        {f.type}
                      </span>
                      {state.ormType === 'prisma' && (
                        <button
                          onClick={() => handleDeleteField(singleSelected.name, fi)}
                          title="Remove field"
                          style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--vscode-foreground)', opacity: 0.35, fontSize: 11, padding: '0 2px', lineHeight: 1, flexShrink: 0 }}
                        >×</button>
                      )}
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
                  {JSON.stringify(modelToJson(singleSelected, state.ormType), null, 2)}
                </pre>
              </>
            )}

            {/* Edge detail panel */}
            {selectedEdge !== null && singleSelected === null && (
              <>
                {/* Header */}
                <div style={{
                  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                  padding: '10px 14px', flexShrink: 0,
                  borderBottom: '1px solid var(--vscode-panel-border,rgba(255,255,255,0.1))',
                }}>
                  <span style={{ fontWeight: 700, fontSize: 13 }}>Relation</span>
                  <button
                    onClick={() => setSelectedEdge(null)}
                    style={{ background: 'none', border: 'none', color: 'var(--vscode-foreground)', fontSize: 14, cursor: 'pointer', opacity: 0.45, padding: 2, lineHeight: 1 }}
                  >✕</button>
                </div>

                {/* Edge info */}
                <div style={{ padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 10 }}>
                  <div style={{ fontSize: 12, opacity: 0.7 }}>
                    <span style={{ fontFamily: 'monospace', color: modelColor(selectedEdge.from) }}>{selectedEdge.from}.{getEdgeRelFieldName(selectedEdge)}</span>
                    <span style={{ margin: '0 6px', opacity: 0.4 }}>→</span>
                    <span style={{ fontFamily: 'monospace', color: modelColor(selectedEdge.to) }}>{selectedEdge.to}</span>
                  </div>

                  {/* Relation type badge + toggle */}
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{
                      fontSize: 10, padding: '2px 8px', borderRadius: 10,
                      background: 'rgba(99,102,241,0.12)', color: '#a5b4fc',
                      border: '1px solid rgba(99,102,241,0.22)',
                    }}>{getEdgeRelType(selectedEdge)}</span>
                    {state.ormType === 'prisma' && (
                      <button
                        onClick={() => handleToggleEdgeRelType(selectedEdge)}
                        style={{
                          ...btnBase, fontSize: 10,
                        }}
                      >Toggle</button>
                    )}
                  </div>

                  {/* Delete button */}
                  {state.ormType === 'prisma' && (
                    <button
                      onClick={() => handleDeleteEdge(selectedEdge)}
                      style={{
                        ...btnBase,
                        borderColor: 'rgba(239,68,68,0.4)',
                        color: '#f87171',
                        fontSize: 11, marginTop: 4,
                      }}
                    >Delete Relation</button>
                  )}
                </div>
              </>
            )}
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
