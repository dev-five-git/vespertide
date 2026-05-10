import React, { useState, useEffect, useRef, useCallback } from 'react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';
import { DEFAULT_SCHEMAS } from '../App';

// ── Types ─────────────────────────────────────────────────────────────────────

type Field = { name: string; type: string; isPrimary: boolean; isRelation: boolean };
type Model = { name: string; fields: Field[] };
type Pos   = { x: number; y: number };
type EdgeDef = {
  from:          string;
  fromFieldIdx:  number;
  to:            string;
  relationField: string;
  fkField:       string;
};

type RelType = 'many-to-one' | 'one-to-many';

type AddRelForm = {
  target:      string;    // target model name
  relType:     RelType;
  relField:    string;    // relation field name added to source model
  fkField:     string;    // FK scalar field name (many-to-one only)
  fkType:      string;    // FK scalar type  e.g. "Int"
  refField:    string;    // referenced field in target  e.g. "id"
  backRef:     string;    // back-reference field name added to target
  addBackRef:  boolean;   // whether to add back-reference
};

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

// ── Prisma source manipulation ────────────────────────────────────────────────

function removeFieldsFromModel(src: string, modelName: string, fieldNames: string[]): string {
  const fieldSet = new Set(fieldNames);
  const lines = src.split('\n');
  let inModel = false;
  let depth = 0;
  return lines.filter((line) => {
    if (!inModel) {
      const match = line.match(/^model\s+(\w+)\s*\{/);
      if (match && match[1] === modelName) { inModel = true; depth = 1; }
      return true;
    }
    depth += (line.match(/\{/g) ?? []).length - (line.match(/\}/g) ?? []).length;
    if (depth <= 0) { inModel = false; return true; }
    const fieldName = line.trim().split(/\s+/)[0];
    return !fieldSet.has(fieldName);
  }).join('\n');
}

function addFieldsToModel(src: string, modelName: string, newFields: string[]): string {
  const lines = src.split('\n');
  let inModel = false;
  let depth = 0;
  const result: string[] = [];
  for (const line of lines) {
    if (!inModel) {
      result.push(line);
      const match = line.match(/^model\s+(\w+)\s*\{/);
      if (match && match[1] === modelName) { inModel = true; depth = 1; }
    } else {
      depth += (line.match(/\{/g) ?? []).length - (line.match(/\}/g) ?? []).length;
      if (depth <= 0) {
        for (const f of newFields) result.push('  ' + f);
        inModel = false;
      }
      result.push(line);
    }
  }
  return result.join('\n');
}

function deletePrismaRelation(
  src: string, modelName: string, relationField: string, fkField: string
): string {
  return removeFieldsFromModel(src, modelName, [relationField, fkField]);
}

function addPrismaRelation(src: string, fromModel: string, form: AddRelForm): string {
  if (form.relType === 'many-to-one') {
    src = addFieldsToModel(src, fromModel, [
      `${form.fkField}   ${form.fkType}`,
      `${form.relField}  ${form.target}  @relation(fields: [${form.fkField}], references: [${form.refField}])`,
    ]);
    if (form.addBackRef && form.backRef) {
      src = addFieldsToModel(src, form.target, [`${form.backRef}  ${fromModel}[]`]);
    }
  } else {
    src = addFieldsToModel(src, fromModel, [`${form.relField}  ${form.target}[]`]);
    if (form.addBackRef && form.backRef) {
      src = addFieldsToModel(src, form.target, [
        `${form.backRef}Id   ${form.fkType}`,
        `${form.backRef}  ${fromModel}  @relation(fields: [${form.backRef}Id], references: [${form.refField}])`,
      ]);
    }
  }
  return src;
}

function defaultAddRelForm(fromModel: string, toModel: string, targetModels: Model[]): AddRelForm {
  const lc = (s: string) => s[0].toLowerCase() + s.slice(1);
  const target = targetModels.find((m) => m.name === toModel);
  const pkField = target?.fields.find((f) => f.isPrimary)?.name ?? 'id';
  return {
    target:     toModel,
    relType:    'many-to-one',
    relField:   lc(toModel),
    fkField:    lc(toModel) + 'Id',
    fkType:     'Int',
    refField:   pkField,
    backRef:    lc(fromModel) + 's',
    addBackRef: true,
  };
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

function getEdges(models: Model[]): EdgeDef[] {
  const names = new Set(models.map((m) => m.name));
  const edges: EdgeDef[] = [];
  for (const model of models) {
    for (let fi = 0; fi < model.fields.length; fi++) {
      const f = model.fields[fi];
      if (!f.isRelation) continue;
      const base = f.type.replace('[]', '').replace('?', '');
      if (!names.has(base) || base === model.name) continue;
      const fkField = model.fields.find(
        (sf) => !sf.isRelation && (sf.name === f.name + 'Id' || sf.name === f.name + '_id')
      );
      if (!fkField) continue;
      edges.push({ from: model.name, fromFieldIdx: fi, to: base, relationField: f.name, fkField: fkField.name });
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
const NODE_W         = 204;
const HEADER_H       = 38;
const FIELD_H        = 23;
const DRAG_THRESHOLD = 5;

function nodeHeight(m: Model) { return HEADER_H + m.fields.length * FIELD_H + 6; }

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

// ── Button styles ─────────────────────────────────────────────────────────────

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
  width: 1, height: 16,
  background: 'var(--vscode-panel-border, rgba(255,255,255,0.12))',
  margin: '0 3px', flexShrink: 0,
};

// ── Component ─────────────────────────────────────────────────────────────────

type Props = { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> };

export default function OrmEditor({ state, setState }: Props) {
  const [models,       setModels]       = useState<Model[]>([]);
  const [positions,    setPositions]    = useState<Record<string, Pos>>({});
  const [selected,     setSelected]     = useState<Model | null>(null);
  const [selectedEdge, setSelectedEdge] = useState<EdgeDef | null>(null);
  const [addRelForm,   setAddRelForm]   = useState<AddRelForm | null>(null);
  const [showCode,     setShowCode]     = useState(false);
  const [lockMode,     setLockMode]     = useState(false);
  const [pan,   setPan]   = useState<Pos>({ x: 32, y: 32 });
  const [scale, setScale] = useState(1);

  const panRef   = useRef(pan);
  const scaleRef = useRef(scale);
  useEffect(() => { panRef.current = pan; },   [pan]);
  useEffect(() => { scaleRef.current = scale; }, [scale]);

  const pendingRef  = useRef<{ id: string; ox: number; oy: number; sx: number; sy: number } | null>(null);
  const draggingRef = useRef<{ id: string; ox: number; oy: number } | null>(null);
  const didDragRef  = useRef(false);
  const panningRef  = useRef<{ mx: number; my: number; px: number; py: number } | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const canvasRef   = useRef<HTMLDivElement>(null);

  // ── Parse source ─────────────────────────────────────────────────────────────

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

  useEffect(() => {
    if (!selected) return;
    const fresh = models.find((m) => m.name === selected.name);
    setSelected(fresh ?? null);
  }, [models]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Mouse handlers ────────────────────────────────────────────────────────────

  const onMouseMove = useCallback((e: MouseEvent) => {
    const { x: px, y: py } = panRef.current;
    const sc = scaleRef.current;

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

  const startNodeDrag = (e: React.MouseEvent, id: string) => {
    if (lockMode) return;
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

  const handleNodeClick = (e: React.MouseEvent, model: Model) => {
    e.stopPropagation();
    if (!lockMode && !didDragRef.current) {
      setSelectedEdge(null);
      setAddRelForm(null);
      setSelected((prev) => (prev?.name === model.name ? null : model));
    }
  };

  const startPan = (e: React.MouseEvent) => {
    panningRef.current = { mx: e.clientX, my: e.clientY, px: pan.x, py: pan.y };
  };

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

  // ── Edge click ────────────────────────────────────────────────────────────────

  const handleEdgeClick = (e: React.MouseEvent, edge: EdgeDef) => {
    if (lockMode) return;
    e.stopPropagation();
    setSelected(null);
    setAddRelForm(null);
    setSelectedEdge((prev) =>
      prev?.from === edge.from && prev?.fromFieldIdx === edge.fromFieldIdx ? null : edge
    );
  };

  // ── Relation editing ──────────────────────────────────────────────────────────

  const handleDeleteRelation = () => {
    if (!selectedEdge || state.ormType !== 'prisma') return;
    const newSrc = deletePrismaRelation(
      state.ormSource, selectedEdge.from, selectedEdge.relationField, selectedEdge.fkField
    );
    setState((p) => ({ ...p, ormSource: newSrc }));
    setSelectedEdge(null);
  };

  const openAddRelForm = (fromModel: string) => {
    const firstTarget = models.find((m) => m.name !== fromModel);
    if (!firstTarget) return;
    setAddRelForm(defaultAddRelForm(fromModel, firstTarget.name, models));
  };

  const handleAddRelation = () => {
    if (!selected || !addRelForm || state.ormType !== 'prisma') return;
    const newSrc = addPrismaRelation(state.ormSource, selected.name, addRelForm);
    setState((p) => ({ ...p, ormSource: newSrc }));
    setAddRelForm(null);
  };

  const patchForm = (patch: Partial<AddRelForm>) =>
    setAddRelForm((f) => f ? { ...f, ...patch } : f);

  // ── Derived ───────────────────────────────────────────────────────────────────

  const edges = getEdges(models);
  const isEdgeSel = (e: EdgeDef) =>
    selectedEdge?.from === e.from && selectedEdge?.fromFieldIdx === e.fromFieldIdx;

  // ── Render ────────────────────────────────────────────────────────────────────

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>

      {/* ── Toolbar ── */}
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
          onMouseDown={startPan}
          onClick={() => { setSelected(null); setSelectedEdge(null); setAddRelForm(null); }}
          style={{
            flex: 1, position: 'relative', overflow: 'hidden',
            cursor: lockMode ? 'grab' : 'default',
            background: 'var(--vscode-editor-background, #1e1e1e)',
          }}
        >
          {/* Dot grid */}
          <div style={{
            position: 'absolute', inset: 0, pointerEvents: 'none',
            backgroundImage: 'radial-gradient(circle, var(--canvas-dot) 1px, transparent 1px)',
            backgroundSize: `${24 * scale}px ${24 * scale}px`,
            backgroundPosition: `${pan.x % (24 * scale)}px ${pan.y % (24 * scale)}px`,
          }} />

          {/* ── SVG for edges — fills full canvas, pan/scale via <g> ── */}
          <svg
            style={{
              position: 'absolute', inset: 0,
              width: '100%', height: '100%',
              pointerEvents: 'none',
              overflow: 'visible',
            }}
          >
            <defs>
              <marker id="vt-arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
                <path d="M0,0.5 L0,5.5 L6.5,3 z" style={{ fill: 'var(--edge-arrow, rgba(99,102,241,0.8))' }} />
              </marker>
              <marker id="vt-arrow-sel" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
                <path d="M0,0.5 L0,5.5 L6.5,3 z" fill="#818cf8" />
              </marker>
            </defs>
            <g transform={`translate(${pan.x},${pan.y}) scale(${scale})`}>
              {edges.map((edge, i) => {
                const from = positions[edge.from];
                const to   = positions[edge.to];
                if (!from || !to) return null;
                const fromModel = models.find((m) => m.name === edge.from);
                const toModel   = models.find((m) => m.name === edge.to);
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
                const d = `M${x1} ${y1} C${c1x} ${y1},${c2x} ${y2},${x2} ${y2}`;
                const sel = isEdgeSel(edge);
                const edgeColor = sel ? '#818cf8' : 'rgba(99,102,241,0.55)';

                return (
                  <g key={i}>
                    {/* Fat transparent hit area */}
                    <path
                      d={d}
                      fill="none"
                      stroke="transparent"
                      strokeWidth={14}
                      style={{ pointerEvents: lockMode ? 'none' : 'stroke', cursor: lockMode ? 'grab' : 'pointer' }}
                      onClick={(e) => handleEdgeClick(e, edge)}
                    />
                    {/* Visible path */}
                    <path
                      d={d}
                      fill="none"
                      stroke={edgeColor}
                      strokeWidth={sel ? 2.5 : 1.5}
                      strokeDasharray={sel ? undefined : undefined}
                      markerEnd={sel ? 'url(#vt-arrow-sel)' : 'url(#vt-arrow)'}
                      style={{ pointerEvents: 'none' }}
                    />
                    {/* Origin dot */}
                    <circle
                      cx={x1} cy={y1} r={sel ? 4 : 3}
                      fill={edgeColor}
                      style={{ pointerEvents: 'none' }}
                    />
                  </g>
                );
              })}
            </g>
          </svg>

          {/* ── Node divs (same z-layer, after SVG) ── */}
          <div style={{
            position: 'absolute', transformOrigin: '0 0',
            transform: `translate(${pan.x}px,${pan.y}px) scale(${scale})`,
          }}>
            {models.map((model) => {
              const pos   = positions[model.name] ?? { x: 0, y: 0 };
              const color = modelColor(model.name);
              const isSel = selected?.name === model.name;
              const hasSelEdge = selectedEdge?.from === model.name || selectedEdge?.to === model.name;
              return (
                <div key={model.name}
                  onMouseDown={(e) => startNodeDrag(e, model.name)}
                  onClick={(e) => handleNodeClick(e, model)}
                  style={{
                    position: 'absolute', left: pos.x, top: pos.y,
                    width: NODE_W, height: nodeHeight(model),
                    borderRadius: 8, overflow: 'hidden', userSelect: 'none', cursor: lockMode ? 'grab' : 'pointer',
                    border: `1.5px solid ${isSel ? color : hasSelEdge ? color + '88' : 'var(--node-border)'}`,
                    boxShadow: isSel
                      ? `0 0 0 3px ${color}28, 0 4px 18px rgba(0,0,0,0.35)`
                      : hasSelEdge ? `0 0 0 2px ${color}18` : 'var(--node-shadow)',
                    background: 'var(--node-bg)',
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
                      borderBottom: fi < model.fields.length - 1
                        ? '1px solid var(--node-field-divider)' : 'none',
                    }}>
                      <span style={{ fontSize: 9, width: 10, textAlign: 'center', flexShrink: 0, opacity: 0.5 }}>
                        {f.isPrimary ? '⬡' : f.isRelation ? '⇢' : '·'}
                      </span>
                      <span style={{ fontSize: 11, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {f.name}
                      </span>
                      <span style={{
                        fontSize: 10, flexShrink: 0, fontFamily: 'monospace',
                        opacity: f.isRelation ? 0.8 : 0.4,
                        color: f.isRelation ? color : 'inherit',
                      }}>
                        {f.type}
                      </span>
                    </div>
                  ))}
                </div>
              );
            })}
          </div>

          {models.length === 0 && (
            <div style={{
              position: 'absolute', inset: 0, display: 'flex',
              alignItems: 'center', justifyContent: 'center',
              opacity: 0.3, pointerEvents: 'none',
            }}>
              <span style={{ fontSize: 12 }}>스키마를 파싱하는 중...</span>
            </div>
          )}

          {/* ── Bottom nav bar ── */}
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
            <button style={navBtn()} title="축소 (10%)"
              onClick={() => setScale((s) => Math.max(0.25, +(s - 0.1).toFixed(2)))}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
                <line x1="5" y1="12" x2="19" y2="12"/>
              </svg>
            </button>

            <button
              style={{ ...navBtn(), width: 44, fontSize: 11, fontVariantNumeric: 'tabular-nums' }}
              title="100%로 초기화"
              onClick={() => { setScale(1); setPan({ x: 32, y: 32 }); }}
            >
              {Math.round(scale * 100)}%
            </button>

            <button style={navBtn()} title="확대 (10%)"
              onClick={() => setScale((s) => Math.min(2, +(s + 0.1).toFixed(2)))}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
                <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
              </svg>
            </button>

            <div style={navDivider} />

            <button style={navBtn()} title="화면에 맞추기"
              onClick={() => { setScale(1); setPan({ x: 32, y: 32 }); }}>
              <IconFit />
            </button>

            <div style={navDivider} />

            <button
              style={navBtn(lockMode)}
              title={lockMode ? '이동 모드 (클릭하여 편집 모드로)' : '편집 모드 (클릭하여 이동 모드로)'}
              onClick={() => { setLockMode((v) => !v); if (lockMode) setSelected(null); }}
            >
              <IconHand />
            </button>

            <div style={navDivider} />

            <button
              style={navBtn()}
              title={state.theme === 'dark' ? '라이트 모드로 전환' : '다크 모드로 전환'}
              onClick={() => setState((p) => ({ ...p, theme: p.theme === 'dark' ? 'light' : 'dark' }))}
            >
              {state.theme === 'dark' ? <IconSun /> : <IconMoon />}
            </button>
          </div>
        </div>

        {/* ── Detail panel (model or relation) ── */}
        {(selected || selectedEdge) && (
          <div style={{
            width: 276, flexShrink: 0,
            borderLeft: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            display: 'flex', flexDirection: 'column',
            background: 'var(--vscode-sideBar-background, #252526)',
            animation: 'slideInRight 0.15s ease-out',
          }}>
            {selected ? (
              /* ── Model detail ── */
              <>
                {/* Header */}
                <div style={{
                  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                  padding: '10px 14px', flexShrink: 0,
                  borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
                }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
                    <div style={{ width: 10, height: 10, borderRadius: '50%', background: modelColor(selected.name), flexShrink: 0 }} />
                    <span style={{ fontWeight: 700, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {selected.name}
                    </span>
                    <span style={{
                      fontSize: 10, padding: '1px 7px', borderRadius: 10, flexShrink: 0,
                      background: 'rgba(99,102,241,0.12)', color: '#a5b4fc',
                      border: '1px solid rgba(99,102,241,0.22)',
                    }}>{state.ormType}</span>
                  </div>
                  <button
                    onClick={(e) => { e.stopPropagation(); setSelected(null); }}
                    style={{ background: 'none', border: 'none', color: 'var(--vscode-foreground)', fontSize: 14, cursor: 'pointer', opacity: 0.45, padding: 2 }}
                  >✕</button>
                </div>

                {/* Fields */}
                <div style={{ padding: '10px 14px', flexShrink: 0, borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
                  {selected.fields.map((f) => (
                    <div key={f.name} style={{ display: 'flex', alignItems: 'baseline', gap: 6, marginBottom: 5 }}>
                      <span style={{ fontSize: 9, width: 12, textAlign: 'center', opacity: 0.4, flexShrink: 0 }}>
                        {f.isPrimary ? '⬡' : f.isRelation ? '⇢' : '·'}
                      </span>
                      <span style={{ fontSize: 12, flex: 1 }}>{f.name}</span>
                      <span style={{
                        fontSize: 10, fontFamily: 'monospace', flexShrink: 0,
                        opacity: f.isRelation ? 0.75 : 0.4,
                        color: f.isRelation
                          ? modelColor(f.type.replace('[]','').replace('?',''))
                          : 'inherit',
                      }}>
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
                  fontFamily: 'var(--vscode-editor-font-family, Consolas, monospace)',
                  fontSize: 11, lineHeight: 1.65,
                  color: 'var(--vscode-editor-foreground, #d4d4d4)',
                  background: 'transparent', whiteSpace: 'pre',
                }}>
                  {JSON.stringify(modelToJson(selected, state.ormType), null, 2)}
                </pre>

                {/* Add Relation — only for Prisma */}
                {state.ormType === 'prisma' && (
                  <div style={{
                    flexShrink: 0,
                    borderTop: '1px solid rgba(255,255,255,0.06)',
                  }}>
                    {!addRelForm ? (
                      <div style={{ padding: '10px 14px' }}>
                        <button
                          onClick={(e) => { e.stopPropagation(); openAddRelForm(selected.name); }}
                          style={{
                            width: '100%', padding: '5px 0',
                            border: '1px dashed rgba(99,102,241,0.4)',
                            borderRadius: 5, background: 'transparent',
                            color: '#a5b4fc', fontSize: 11, cursor: 'pointer',
                          }}
                        >+ Relation 추가</button>
                      </div>
                    ) : (
                      <AddRelFormPanel
                        form={addRelForm}
                        fromModel={selected.name}
                        allModels={models}
                        onPatch={patchForm}
                        onConfirm={(e) => { e.stopPropagation(); handleAddRelation(); }}
                        onCancel={(e) => { e.stopPropagation(); setAddRelForm(null); }}
                      />
                    )}
                  </div>
                )}
              </>
            ) : selectedEdge ? (
              /* ── Relation detail ── */
              <>
                {/* Header */}
                <div style={{
                  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                  padding: '10px 14px', flexShrink: 0,
                  borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
                }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ fontSize: 10, opacity: 0.5 }}>⇢</span>
                    <span style={{ fontWeight: 700, fontSize: 13 }}>Relation</span>
                  </div>
                  <button
                    onClick={(e) => { e.stopPropagation(); setSelectedEdge(null); }}
                    style={{ background: 'none', border: 'none', color: 'var(--vscode-foreground)', fontSize: 14, cursor: 'pointer', opacity: 0.45, padding: 2 }}
                  >✕</button>
                </div>

                {/* Relation info */}
                <div style={{ padding: '14px', flexShrink: 0 }}>
                  {/* From → To */}
                  <div style={{
                    display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16,
                    padding: '8px 12px', borderRadius: 6,
                    background: 'rgba(99,102,241,0.08)',
                    border: '1px solid rgba(99,102,241,0.18)',
                  }}>
                    <span style={{
                      fontWeight: 700, fontSize: 13,
                      color: modelColor(selectedEdge.from),
                    }}>{selectedEdge.from}</span>
                    <span style={{ fontSize: 11, opacity: 0.5 }}>→</span>
                    <span style={{
                      fontWeight: 700, fontSize: 13,
                      color: modelColor(selectedEdge.to),
                    }}>{selectedEdge.to}</span>
                    <span style={{
                      marginLeft: 'auto', fontSize: 9, padding: '1px 6px',
                      borderRadius: 10, background: 'rgba(99,102,241,0.15)',
                      color: '#a5b4fc', border: '1px solid rgba(99,102,241,0.25)',
                    }}>many-to-one</span>
                  </div>

                  {/* Field details */}
                  {[
                    { label: 'relation field', value: selectedEdge.relationField },
                    { label: 'fk field',       value: selectedEdge.fkField },
                  ].map(({ label, value }) => (
                    <div key={label} style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 8, fontSize: 12 }}>
                      <span style={{ opacity: 0.45, fontSize: 11 }}>{label}</span>
                      <span style={{ fontFamily: 'monospace', fontSize: 11 }}>{value}</span>
                    </div>
                  ))}
                </div>

                <div style={{ flex: 1 }} />

                {/* Delete button — only for Prisma */}
                {state.ormType === 'prisma' && (
                  <div style={{ padding: '10px 14px', flexShrink: 0, borderTop: '1px solid rgba(255,255,255,0.06)' }}>
                    <button
                      onClick={(e) => { e.stopPropagation(); handleDeleteRelation(); }}
                      style={{
                        width: '100%', padding: '6px 0',
                        border: '1px solid rgba(248,113,113,0.35)',
                        borderRadius: 5, background: 'rgba(248,113,113,0.08)',
                        color: '#f87171', fontSize: 11, cursor: 'pointer',
                      }}
                    >Relation 삭제</button>
                  </div>
                )}
              </>
            ) : null}
          </div>
        )}
      </div>

      {/* ── Code drawer ── */}
      {showCode && (
        <div style={{
          height: 220, flexShrink: 0,
          borderTop: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
          display: 'flex', flexDirection: 'column',
          animation: 'slideInUp 0.15s ease-out',
        }}>
          <div style={{
            display: 'flex', gap: 4, padding: '5px 10px', flexShrink: 0, alignItems: 'center',
            borderBottom: '1px solid rgba(255,255,255,0.05)',
          }}>
            {ORM_TYPES.map((orm) => (
              <button key={orm}
                onClick={() => setState((p) => ({ ...p, ormType: orm, ormSource: DEFAULT_SCHEMAS[orm] }))}
                style={{
                  padding: '2px 8px', border: '1px solid', borderRadius: 3, fontSize: 10, cursor: 'pointer',
                  borderColor: state.ormType === orm ? 'var(--vscode-focusBorder, #007acc)' : 'rgba(255,255,255,0.15)',
                  background:  state.ormType === orm ? 'var(--vscode-button-background, #0e639c)' : 'transparent',
                  color:       state.ormType === orm ? 'var(--vscode-button-foreground, #fff)' : 'var(--vscode-foreground)',
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
              fontFamily: 'var(--vscode-editor-font-family, Consolas, monospace)',
              fontSize: 12, color: 'var(--vscode-editor-foreground, #d4d4d4)',
              background: 'var(--vscode-editor-background, #1e1e1e)',
              lineHeight: 1.6,
            }}
          />
        </div>
      )}
    </div>
  );
}

// ── AddRelFormPanel ───────────────────────────────────────────────────────────

const FK_TYPES = ['Int', 'String', 'BigInt'];

const inputStyle: React.CSSProperties = {
  width: '100%', padding: '3px 6px', borderRadius: 3, fontSize: 11,
  background: 'var(--vscode-input-background, #3c3c3c)',
  color: 'var(--vscode-foreground)',
  border: '1px solid var(--vscode-input-border, rgba(255,255,255,0.18))',
  outline: 'none',
};

const labelStyle: React.CSSProperties = {
  fontSize: 10, opacity: 0.45, marginBottom: 2, display: 'block',
};

function AddRelFormPanel({
  form, fromModel, allModels, onPatch, onConfirm, onCancel,
}: {
  form:       AddRelForm;
  fromModel:  string;
  allModels:  Model[];
  onPatch:    (patch: Partial<AddRelForm>) => void;
  onConfirm:  (e: React.MouseEvent) => void;
  onCancel:   (e: React.MouseEvent) => void;
}) {
  const otherModels = allModels.filter((m) => m.name !== fromModel);
  const targetModel = allModels.find((m) => m.name === form.target);
  const scalarFields = targetModel?.fields.filter((f) => !f.isRelation) ?? [];

  const handleTargetChange = (name: string) => {
    const lc = (s: string) => s[0].toLowerCase() + s.slice(1);
    const t = allModels.find((m) => m.name === name);
    const pkField = t?.fields.find((f) => f.isPrimary)?.name ?? 'id';
    onPatch({
      target:   name,
      relField: lc(name),
      fkField:  lc(name) + 'Id',
      refField: pkField,
    });
  };

  const handleRelTypeChange = (relType: RelType) => {
    const lc = (s: string) => s[0].toLowerCase() + s.slice(1);
    if (relType === 'many-to-one') {
      onPatch({ relType, relField: lc(form.target), fkField: lc(form.target) + 'Id', backRef: lc(fromModel) + 's' });
    } else {
      onPatch({ relType, relField: lc(form.target) + 's', backRef: lc(fromModel) });
    }
  };

  const Row = ({ label, children }: { label: string; children: React.ReactNode }) => (
    <div style={{ marginBottom: 8 }}>
      <span style={labelStyle}>{label}</span>
      {children}
    </div>
  );

  return (
    <div style={{ padding: '10px 14px' }} onClick={(e) => e.stopPropagation()}>
      {/* Header */}
      <div style={{ fontSize: 10, fontWeight: 700, opacity: 0.4, letterSpacing: '0.06em', marginBottom: 10 }}>
        NEW RELATION
      </div>

      {/* Target model */}
      <Row label="연결할 모델">
        <select value={form.target} onChange={(e) => handleTargetChange(e.target.value)} style={inputStyle}>
          {otherModels.map((m) => <option key={m.name} value={m.name}>{m.name}</option>)}
        </select>
      </Row>

      {/* Relation type */}
      <Row label="방향">
        <div style={{ display: 'flex', gap: 4 }}>
          {(['many-to-one', 'one-to-many'] as RelType[]).map((t) => (
            <button key={t} onClick={() => handleRelTypeChange(t)} style={{
              flex: 1, padding: '3px 0', fontSize: 10, borderRadius: 3,
              border: '1px solid',
              borderColor: form.relType === t ? 'var(--vscode-focusBorder,#007acc)' : 'rgba(255,255,255,0.15)',
              background:  form.relType === t ? 'rgba(0,122,204,0.15)' : 'transparent',
              color: 'var(--vscode-foreground)', cursor: 'pointer',
            }}>{t === 'many-to-one' ? `${fromModel} → ${form.target}` : `${fromModel} ← ${form.target}`}</button>
          ))}
        </div>
      </Row>

      {/* Relation field name */}
      <Row label="relation 필드명 (이 모델에 추가)">
        <input style={inputStyle} value={form.relField}
          onChange={(e) => onPatch({ relField: e.target.value })} />
      </Row>

      {/* FK field (many-to-one only) */}
      {form.relType === 'many-to-one' && (
        <Row label="FK 필드명">
          <div style={{ display: 'flex', gap: 4 }}>
            <input style={{ ...inputStyle, flex: 1 }} value={form.fkField}
              onChange={(e) => onPatch({ fkField: e.target.value })} />
            <select value={form.fkType} onChange={(e) => onPatch({ fkType: e.target.value })}
              style={{ ...inputStyle, width: 60 }}>
              {FK_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
            </select>
          </div>
        </Row>
      )}

      {/* Referenced field */}
      <Row label={`참조 필드 (${form.target})`}>
        <select value={form.refField} onChange={(e) => onPatch({ refField: e.target.value })} style={inputStyle}>
          {scalarFields.map((f) => <option key={f.name} value={f.name}>{f.name}</option>)}
        </select>
      </Row>

      {/* Back-reference */}
      <div style={{ marginBottom: 8 }}>
        <label style={{ ...labelStyle, display: 'flex', alignItems: 'center', gap: 6, cursor: 'pointer', opacity: 1 }}>
          <input
            type="checkbox"
            checked={form.addBackRef}
            onChange={(e) => onPatch({ addBackRef: e.target.checked })}
            style={{ accentColor: '#6366f1' }}
          />
          <span style={{ fontSize: 10, opacity: 0.45 }}>역참조 추가 ({form.target} 모델에)</span>
        </label>
        {form.addBackRef && (
          <input style={{ ...inputStyle, marginTop: 4 }} value={form.backRef}
            onChange={(e) => onPatch({ backRef: e.target.value })} />
        )}
      </div>

      {/* Actions */}
      <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
        <button onClick={onConfirm} style={{
          flex: 1, padding: '5px 0', borderRadius: 3, border: 'none',
          background: 'var(--vscode-button-background, #0e639c)',
          color: 'var(--vscode-button-foreground, #fff)',
          fontSize: 11, cursor: 'pointer',
        }}>추가</button>
        <button onClick={onCancel} style={{
          padding: '5px 12px', borderRadius: 3,
          border: '1px solid rgba(255,255,255,0.15)',
          background: 'transparent', color: 'var(--vscode-foreground)',
          fontSize: 11, cursor: 'pointer',
        }}>취소</button>
      </div>
    </div>
  );
}
