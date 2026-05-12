import React, { useState, useEffect, useRef, useCallback } from 'react';
import { postMessage } from '../vscode';
import type { OrmType } from '../vscode';
import type { AppState } from '../App';
import { DEFAULT_SCHEMAS } from '../App';

// ── Types ─────────────────────────────────────────────────────────────────────

type Field = { name: string; type: string; isPrimary: boolean; isRelation: boolean; refField?: string };
type Model = { name: string; fields: Field[] };
type Pos   = { x: number; y: number };
type EdgeDef = {
  from:          string;
  fromFieldIdx:  number;
  to:            string;
  relationField: string;
  fkField:       string;
  refField:      string;   // referenced field in target model (e.g. "id")
  toFieldIdx:    number;   // index of refField in target model's fields array
};

type DraggingEndpoint = {
  edge: EdgeDef;
  x: number;  // canvas-space (before pan/scale)
  y: number;
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
        const parts    = l.split(/\s+/);
        const base     = (parts[1] ?? '').replace('[]', '').replace('?', '');
        const refMatch = l.match(/@relation\([^)]*references:\s*\[([^\]]+)\]/);
        return {
          name:       parts[0],
          type:       parts[1] ?? '',
          isPrimary:  l.includes('@id'),
          isRelation: !PRISMA_SCALARS.has(base) && /^[A-Z]/.test(base),
          refField:   refMatch ? refMatch[1].split(',')[0].trim() : undefined,
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
      const tgt          = models.find((m) => m.name === base);
      const refFieldName = f.refField ?? tgt?.fields.find((tf) => tf.isPrimary)?.name ?? 'id';
      const toFieldIdx   = Math.max(0, tgt?.fields.findIndex((tf) => tf.name === refFieldName) ?? 0);
      edges.push({
        from: model.name, fromFieldIdx: fi, to: base,
        relationField: f.name, fkField: fkField.name,
        refField: refFieldName, toFieldIdx,
      });
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

// ── Endpoint reroute helpers ──────────────────────────────────────────────────

/** Returns which model+field is at canvas-space point (x, y), null if none. */
function getFieldAtPoint(
  models: Model[],
  positions: Record<string, Pos>,
  x: number,
  y: number,
): { model: string; field: string; fieldIdx: number } | null {
  for (const model of models) {
    const pos = positions[model.name];
    if (!pos) continue;
    if (x < pos.x || x > pos.x + NODE_W) continue;
    const relY = y - pos.y - HEADER_H;
    if (relY < 0 || relY > model.fields.length * FIELD_H) continue;
    const fieldIdx = Math.floor(relY / FIELD_H);
    const field = model.fields[fieldIdx];
    if (!field) continue;
    return { model: model.name, field: field.name, fieldIdx };
  }
  return null;
}

// ── Snap helpers ─────────────────────────────────────────────────────────────

const SNAP_THRESHOLD = 28;

type SnapPoint = { model: string; field: string; fieldIdx: number; x: number; y: number };

/** All connection points (left + right edge of every field row). */
function getSnapPoints(models: Model[], positions: Record<string, Pos>): SnapPoint[] {
  const pts: SnapPoint[] = [];
  for (const m of models) {
    const pos = positions[m.name];
    if (!pos) continue;
    for (let fi = 0; fi < m.fields.length; fi++) {
      const cy = pos.y + HEADER_H + fi * FIELD_H + FIELD_H / 2;
      pts.push({ model: m.name, field: m.fields[fi].name, fieldIdx: fi, x: pos.x,          y: cy });
      pts.push({ model: m.name, field: m.fields[fi].name, fieldIdx: fi, x: pos.x + NODE_W, y: cy });
    }
  }
  return pts;
}

/** Nearest snap point within SNAP_THRESHOLD, or null. */
function findNearestSnap(pts: SnapPoint[], x: number, y: number): SnapPoint | null {
  let best: SnapPoint | null = null;
  let bestD = SNAP_THRESHOLD;
  for (const pt of pts) {
    const d = Math.hypot(pt.x - x, pt.y - y);
    if (d < bestD) { bestD = d; best = pt; }
  }
  return best;
}

/** Update @relation(references: [...]) and the type token in the relation field line. */
function reroutePrismaRelation(
  src: string,
  fromModel: string,
  relationField: string,
  newToModel: string,
  newRefField: string,
): string {
  const lines = src.split('\n');
  let inModel = false;
  let depth = 0;
  return lines.map((line) => {
    if (!inModel) {
      if (new RegExp(`^model\\s+${fromModel}\\s*\\{`).test(line)) { inModel = true; depth = 1; }
      return line;
    }
    depth += (line.match(/\{/g) ?? []).length - (line.match(/\}/g) ?? []).length;
    if (depth <= 0) { inModel = false; return line; }

    const parts = line.trim().split(/\s+/);
    if (parts[0] !== relationField) return line;

    // Update type (e.g. User → Category)
    const oldType = (parts[1] ?? '').replace(/[?[\]]/g, '');
    let updated = line;
    if (oldType !== newToModel) {
      updated = updated.replace(
        new RegExp(`\\b${oldType.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\b`),
        newToModel,
      );
    }
    // Update references: [...]
    if (updated.includes('@relation(')) {
      updated = updated.replace(
        /references:\s*\[[^\]]*\]/,
        `references: [${newRefField}]`,
      );
    }
    return updated;
  }).join('\n');
}

// ── Layout algorithms ─────────────────────────────────────────────────────────

type LayoutType = 'lr' | 'snowflake' | 'compact';

function layoutLeftRight(models: Model[], edges: EdgeDef[]): Record<string, Pos> {
  const COL_GAP = 80, ROW_GAP = 36, OFF = 40;
  const successors = new Map<string, string[]>();
  const inDeg      = new Map<string, number>();
  for (const m of models) { successors.set(m.name, []); inDeg.set(m.name, 0); }
  for (const e of edges) {
    successors.get(e.from)?.push(e.to);
    inDeg.set(e.to, (inDeg.get(e.to) ?? 0) + 1);
  }

  // Longest-path column assignment
  const col   = new Map<string, number>();
  const queue: string[] = [];
  for (const m of models) {
    if ((inDeg.get(m.name) ?? 0) === 0) { col.set(m.name, 0); queue.push(m.name); }
  }
  if (queue.length === 0) models.forEach((m) => { col.set(m.name, 0); queue.push(m.name); });

  for (let i = 0; i < queue.length; i++) {
    const d = col.get(queue[i]) ?? 0;
    for (const next of successors.get(queue[i]) ?? []) {
      if ((col.get(next) ?? -1) < d + 1) {
        col.set(next, d + 1);
        if (!queue.includes(next)) queue.push(next);
      }
    }
  }
  let maxC = 0;
  for (const c of col.values()) maxC = Math.max(maxC, c);
  for (const m of models) if (!col.has(m.name)) col.set(m.name, maxC + 1);

  // Group & position
  const groups = new Map<number, string[]>();
  for (const [name, c] of col) {
    if (!groups.has(c)) groups.set(c, []);
    groups.get(c)!.push(name);
  }
  for (const names of groups.values()) names.sort();

  const pos: Record<string, Pos> = {};
  for (const [c, names] of groups) {
    const x = OFF + c * (NODE_W + COL_GAP);
    let y = OFF;
    for (const name of names) {
      pos[name] = { x, y };
      y += nodeHeight(models.find((m) => m.name === name)!) + ROW_GAP;
    }
  }
  return pos;
}

function layoutSnowflake(models: Model[], edges: EdgeDef[]): Record<string, Pos> {
  if (models.length === 0) return {};
  const degree = new Map<string, number>();
  for (const m of models) degree.set(m.name, 0);
  for (const e of edges) {
    degree.set(e.from, (degree.get(e.from) ?? 0) + 1);
    degree.set(e.to,   (degree.get(e.to)   ?? 0) + 1);
  }
  const sorted = [...models].sort((a, b) =>
    ((degree.get(b.name) ?? 0) - (degree.get(a.name) ?? 0)) || a.name.localeCompare(b.name)
  );

  const CX = 420, CY = 320;
  const pos: Record<string, Pos> = {};
  pos[sorted[0].name] = { x: CX - NODE_W / 2, y: CY - nodeHeight(sorted[0]) / 2 };

  const RADII = [240, 460, 680];
  let idx = 1;
  for (const r of RADII) {
    if (idx >= sorted.length) break;
    const cap = Math.min(
      sorted.length - idx,
      Math.max(1, Math.floor((2 * Math.PI * r) / (NODE_W + 50)))
    );
    const step = (2 * Math.PI) / cap;
    for (let i = 0; i < cap && idx < sorted.length; i++, idx++) {
      const angle = -Math.PI / 2 + i * step;
      const m = sorted[idx];
      pos[m.name] = {
        x: CX + r * Math.cos(angle) - NODE_W / 2,
        y: CY + r * Math.sin(angle) - nodeHeight(m) / 2,
      };
    }
  }
  return pos;
}

function layoutCompact(models: Model[], _edges: EdgeDef[]): Record<string, Pos> {
  if (models.length === 0) return {};
  const COL_GAP = 40, ROW_GAP = 36, OFF = 40;
  const cols = Math.ceil(Math.sqrt(models.length));
  const pos: Record<string, Pos> = {};

  for (let i = 0; i < models.length; i++) {
    const c = i % cols;
    const r = Math.floor(i / cols);
    // Row Y uses tallest node in row as row height
    const rowStart = r * cols;
    const rowH = Math.max(...models.slice(rowStart, rowStart + cols).map(nodeHeight));
    const y = OFF + Array.from({ length: r }, (_, ri) => {
      const rs = ri * cols;
      return Math.max(...models.slice(rs, rs + cols).map(nodeHeight)) + ROW_GAP;
    }).reduce((s, v) => s + v, 0);
    pos[models[i].name] = { x: OFF + c * (NODE_W + COL_GAP), y };
    void rowH;
  }
  return pos;
}

// ── Icons ─────────────────────────────────────────────────────────────────────

const IconLayout = () => (
  <svg width="13" height="13" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
    <rect x="1"  y="1"  width="5" height="5" rx="1"/>
    <rect x="14" y="1"  width="5" height="5" rx="1"/>
    <rect x="7"  y="13" width="6" height="6" rx="1"/>
    <line x1="6"  y1="3.5" x2="14" y2="3.5"/>
    <line x1="10" y1="6"   x2="10" y2="13"/>
  </svg>
);

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
  const [selectedNodes, setSelectedNodes] = useState<Set<string>>(new Set());
  const [selectedEdge, setSelectedEdge] = useState<EdgeDef | null>(null);
  const [addRelForm,      setAddRelForm]      = useState<AddRelForm | null>(null);
  const [showLayoutMenu,    setShowLayoutMenu]    = useState(false);
  const [draggingEndpoint,  setDraggingEndpoint]  = useState<DraggingEndpoint | null>(null);
  const [rubberBand, setRubberBand] = useState<{x1:number;y1:number;x2:number;y2:number}|null>(null);
  const [showCode,     setShowCode]     = useState(false);
  const [lockMode,     setLockMode]     = useState(false);
  const [pan,   setPan]   = useState<Pos>({ x: 32, y: 32 });
  const [scale, setScale] = useState(1);

  const panRef   = useRef(pan);
  const scaleRef = useRef(scale);
  useEffect(() => { panRef.current = pan; },   [pan]);
  useEffect(() => { scaleRef.current = scale; }, [scale]);

  type DragState = { ids: string[]; offsets: Record<string, {ox:number;oy:number}>; sx:number; sy:number };
  const pendingRef  = useRef<DragState | null>(null);
  const draggingRef = useRef<DragState | null>(null);
  const didDragRef  = useRef(false);
  const panningRef  = useRef<{ mx: number; my: number; px: number; py: number } | null>(null);
  const debounceRef        = useRef<ReturnType<typeof setTimeout> | null>(null);
  const draggingEpRef      = useRef<DraggingEndpoint | null>(null);
  const modelsRef          = useRef(models);
  const positionsRef       = useRef(positions);
  const ormSourceRef       = useRef(state.ormSource);
  const setOrmSource       = useCallback((src: string) => setState((p) => ({ ...p, ormSource: src })), [setState]);
  const setOrmSourceRef    = useRef(setOrmSource);
  const selectedNodesRef   = useRef(selectedNodes);
  const rubberBandRef      = useRef<{sx:number;sy:number}|null>(null);
  const rbDidDragRef       = useRef(false);
  useEffect(() => { modelsRef.current      = models; },          [models]);
  useEffect(() => { positionsRef.current   = positions; },       [positions]);
  useEffect(() => { ormSourceRef.current   = state.ormSource; }, [state.ormSource]);
  useEffect(() => { setOrmSourceRef.current = setOrmSource; },   [setOrmSource]);
  useEffect(() => { selectedNodesRef.current = selectedNodes; }, [selectedNodes]);
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

    if (draggingEpRef.current) {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (rect) {
        const cx = (e.clientX - rect.left - px) / sc;
        const cy = (e.clientY - rect.top  - py) / sc;
        setDraggingEndpoint({ edge: draggingEpRef.current.edge, x: cx, y: cy });
      }
      return;
    }

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
      const { ids, offsets } = draggingRef.current;
      setPositions((prev) => {
        const next = { ...prev };
        for (const nodeId of ids) {
          const { ox, oy } = offsets[nodeId];
          next[nodeId] = { x: (e.clientX - px) / sc - ox, y: (e.clientY - py) / sc - oy };
        }
        return next;
      });
    } else if (panningRef.current) {
      const { mx, my, px: ppx, py: ppy } = panningRef.current;
      setPan({ x: ppx + e.clientX - mx, y: ppy + e.clientY - my });
    }

    // Rubber-band selection rect
    if (rubberBandRef.current) {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (rect) {
        const { sx, sy } = rubberBandRef.current;
        const ex = e.clientX - rect.left;
        const ey = e.clientY - rect.top;
        if (Math.abs(ex - sx) > DRAG_THRESHOLD || Math.abs(ey - sy) > DRAG_THRESHOLD) {
          rbDidDragRef.current = true;
          setRubberBand({ x1: sx, y1: sy, x2: ex, y2: ey });
        }
      }
    }
  }, []);

  const onMouseUp = useCallback((e: MouseEvent) => {
    if (draggingEpRef.current) {
      const ep = draggingEpRef.current;
      draggingEpRef.current = null;
      setDraggingEndpoint(null);

      const rect = canvasRef.current?.getBoundingClientRect();
      if (rect) {
        const { x: px, y: py } = panRef.current;
        const sc = scaleRef.current;
        const cx = (e.clientX - rect.left - px) / sc;
        const cy = (e.clientY - rect.top  - py) / sc;
        const snapTarget = findNearestSnap(
          getSnapPoints(modelsRef.current, positionsRef.current), cx, cy
        );
        const hit = snapTarget
          ? { model: snapTarget.model, field: snapTarget.field, fieldIdx: snapTarget.fieldIdx }
          : getFieldAtPoint(modelsRef.current, positionsRef.current, cx, cy);
        if (hit && (hit.model !== ep.edge.to || hit.field !== ep.edge.refField)) {
          const newSrc = reroutePrismaRelation(
            ormSourceRef.current, ep.edge.from, ep.edge.relationField,
            hit.model, hit.field,
          );
          setOrmSourceRef.current(newSrc);
        }
      }
    }

    // Finalize rubber-band selection
    if (rubberBandRef.current && rbDidDragRef.current) {
      const { sx, sy } = rubberBandRef.current;
      const rect = canvasRef.current?.getBoundingClientRect();
      if (rect) {
        const ex = e.clientX - rect.left;
        const ey = e.clientY - rect.top;
        const { x: px, y: py } = panRef.current;
        const sc = scaleRef.current;
        const cx1 = (Math.min(sx, ex) - px) / sc;
        const cy1 = (Math.min(sy, ey) - py) / sc;
        const cx2 = (Math.max(sx, ex) - px) / sc;
        const cy2 = (Math.max(sy, ey) - py) / sc;
        const hit = new Set<string>();
        for (const m of modelsRef.current) {
          const pos = positionsRef.current[m.name];
          if (!pos) continue;
          if (pos.x < cx2 && pos.x + NODE_W > cx1 &&
              pos.y < cy2 && pos.y + nodeHeight(m) > cy1) {
            hit.add(m.name);
          }
        }
        if (hit.size > 0) {
          setSelectedNodes(hit);
          setSelected(null);
        }
      }
    }
    rubberBandRef.current = null;
    // rbDidDragRef stays true until onClick resets it, so the canvas click
    // handler doesn't immediately clear the selection we just made
    setRubberBand(null);

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
    const sc  = scaleRef.current;
    const { x: px, y: py } = panRef.current;

    // If this node is part of a multi-selection, drag all selected nodes together
    const inSel = selectedNodesRef.current.has(id);
    const ids = (!e.shiftKey && inSel && selectedNodesRef.current.size > 1)
      ? [...selectedNodesRef.current]
      : [id];

    const offsets: Record<string, {ox:number;oy:number}> = {};
    for (const nodeId of ids) {
      const pos = positionsRef.current[nodeId] ?? { x: 0, y: 0 };
      offsets[nodeId] = {
        ox: (e.clientX - px) / sc - pos.x,
        oy: (e.clientY - py) / sc - pos.y,
      };
    }
    pendingRef.current = { ids, offsets, sx: e.clientX, sy: e.clientY };
  };

  const handleNodeClick = (e: React.MouseEvent, model: Model) => {
    e.stopPropagation();
    if (!lockMode && !didDragRef.current) {
      setSelectedEdge(null);
      setAddRelForm(null);
      if (e.shiftKey) {
        setSelectedNodes((prev) => {
          const next = new Set(prev);
          if (next.has(model.name)) next.delete(model.name);
          else next.add(model.name);
          return next;
        });
      } else {
        setSelectedNodes(new Set([model.name]));
        setSelected((prev) => (prev?.name === model.name ? null : model));
      }
    }
  };

  const startPan = (e: React.MouseEvent) => {
    panningRef.current = { mx: e.clientX, my: e.clientY, px: pan.x, py: pan.y };
  };

  const startRubberBand = (e: React.MouseEvent) => {
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    rbDidDragRef.current = false;
    rubberBandRef.current = { sx: e.clientX - rect.left, sy: e.clientY - rect.top };
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

  const startEndpointDrag = (e: React.MouseEvent, edge: EdgeDef) => {
    if (lockMode) return;
    e.stopPropagation();
    e.preventDefault();
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const { x: px, y: py } = panRef.current;
    const sc = scaleRef.current;
    const cx = (e.clientX - rect.left - px) / sc;
    const cy = (e.clientY - rect.top  - py) / sc;
    draggingEpRef.current = { edge, x: cx, y: cy };
    setDraggingEndpoint({ edge, x: cx, y: cy });
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

  const applyLayout = (type: LayoutType) => {
    const fn = type === 'lr' ? layoutLeftRight
             : type === 'snowflake' ? layoutSnowflake
             : layoutCompact;
    setPositions(fn(models, edges));
    setScale(1);
    setPan({ x: 40, y: 40 });
    setShowLayoutMenu(false);
  };

  // ── Derived ───────────────────────────────────────────────────────────────────

  const edges = getEdges(models);
  const isEdgeSel = (e: EdgeDef) =>
    selectedEdge?.from === e.from && selectedEdge?.fromFieldIdx === e.fromFieldIdx;

  const snapPts = draggingEndpoint ? getSnapPoints(models, positions) : null;
  const nearestSnap = snapPts && draggingEndpoint
    ? findNearestSnap(snapPts, draggingEndpoint.x, draggingEndpoint.y)
    : null;

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
          onMouseDown={(e) => lockMode ? startPan(e) : startRubberBand(e)}
          onClick={() => {
            if (rbDidDragRef.current) { rbDidDragRef.current = false; return; }
            setSelected(null); setSelectedNodes(new Set());
            setSelectedEdge(null); setAddRelForm(null); setShowLayoutMenu(false);
          }}
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
            {/* Rubber-band selection rect — screen space, outside pan/scale group */}
            {rubberBand && (
              <rect
                x={Math.min(rubberBand.x1, rubberBand.x2)}
                y={Math.min(rubberBand.y1, rubberBand.y2)}
                width={Math.abs(rubberBand.x2 - rubberBand.x1)}
                height={Math.abs(rubberBand.y2 - rubberBand.y1)}
                fill="rgba(99,102,241,0.07)"
                stroke="#818cf8"
                strokeWidth={1}
                strokeDasharray="4 2"
                style={{ pointerEvents: 'none' }}
              />
            )}

            <g transform={`translate(${pan.x},${pan.y}) scale(${scale})`}>
              {/* Field row highlight — prefers nearest snap, falls back to cursor hit */}
              {draggingEndpoint && (() => {
                const target = nearestSnap
                  ?? getFieldAtPoint(models, positions, draggingEndpoint.x, draggingEndpoint.y);
                if (!target) return null;
                const pos = positions[target.model];
                if (!pos) return null;
                return (
                  <rect
                    x={pos.x} y={pos.y + HEADER_H + target.fieldIdx * FIELD_H}
                    width={NODE_W} height={FIELD_H}
                    fill="#818cf8" fillOpacity={0.18} rx={2}
                    style={{ pointerEvents: 'none' }}
                  />
                );
              })()}

              {edges.map((edge, i) => {
                const from = positions[edge.from];
                const to   = positions[edge.to];
                if (!from || !to) return null;
                const fromModel = models.find((m) => m.name === edge.from);
                const toModel   = models.find((m) => m.name === edge.to);
                if (!fromModel || !toModel) return null;

                const y1 = from.y + HEADER_H + edge.fromFieldIdx * FIELD_H + FIELD_H / 2;
                const y2 = to.y + HEADER_H + edge.toFieldIdx * FIELD_H + FIELD_H / 2;
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
                const isDraggingThis = draggingEndpoint !== null &&
                  draggingEndpoint.edge.from === edge.from &&
                  draggingEndpoint.edge.fromFieldIdx === edge.fromFieldIdx;
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
                    {/* Visible path (dashed when its endpoint is being dragged) */}
                    <path
                      d={d}
                      fill="none"
                      stroke={edgeColor}
                      strokeWidth={sel ? 2.5 : 1.5}
                      strokeDasharray={isDraggingThis ? '5 3' : undefined}
                      strokeOpacity={isDraggingThis ? 0.4 : 1}
                      markerEnd={isDraggingThis ? undefined : (sel ? 'url(#vt-arrow-sel)' : 'url(#vt-arrow)')}
                      style={{ pointerEvents: 'none' }}
                    />
                    {/* Ghost line — snaps to nearest snap point when within threshold */}
                    {isDraggingThis && (
                      <line
                        x1={x1} y1={y1}
                        x2={nearestSnap?.x ?? draggingEndpoint!.x}
                        y2={nearestSnap?.y ?? draggingEndpoint!.y}
                        stroke="#818cf8" strokeWidth={2}
                        strokeDasharray="6 3"
                        markerEnd="url(#vt-arrow-sel)"
                        style={{ pointerEvents: 'none' }}
                      />
                    )}
                    {/* Cardinality badge "N" at source */}
                    {!isDraggingThis && (
                      <g style={{ pointerEvents: 'none' }}>
                        <circle
                          cx={x1 + (useRight ? 13 : -13)} cy={y1}
                          r={8}
                          fill="var(--node-bg)"
                          stroke={edgeColor}
                          strokeWidth={1.2}
                        />
                        <text
                          x={x1 + (useRight ? 13 : -13)} y={y1}
                          textAnchor="middle" dominantBaseline="central"
                          fontSize={8} fontWeight="700"
                          fill={edgeColor}
                        >N</text>
                      </g>
                    )}
                    {/* Cardinality badge "1" at target */}
                    {!isDraggingThis && (
                      <g style={{ pointerEvents: 'none' }}>
                        <circle
                          cx={x2 + (useRight ? -13 : 13)} cy={y2}
                          r={8}
                          fill="var(--node-bg)"
                          stroke={edgeColor}
                          strokeWidth={1.2}
                        />
                        <text
                          x={x2 + (useRight ? -13 : 13)} y={y2}
                          textAnchor="middle" dominantBaseline="central"
                          fontSize={8} fontWeight="700"
                          fill={edgeColor}
                        >1</text>
                      </g>
                    )}
                    {/* Endpoint drag handle — visible when selected, not currently dragging */}
                    {sel && !draggingEndpoint && (
                      <circle
                        cx={x2} cy={y2} r={5}
                        fill="#818cf8" stroke="var(--vscode-editor-background,#1e1e1e)" strokeWidth={1.5}
                        style={{ pointerEvents: 'all', cursor: 'grab' }}
                        onMouseDown={(e) => startEndpointDrag(e, edge)}
                      />
                    )}
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
              const isSel = selectedNodes.has(model.name) || selected?.name === model.name;
              const hasSelEdge = selectedEdge?.from === model.name || selectedEdge?.to === model.name;
              return (
                <div key={model.name}
                  onMouseDown={(e) => startNodeDrag(e, model.name)}
                  onClick={(e) => handleNodeClick(e, model)}
                  style={{
                    position: 'absolute', left: pos.x, top: pos.y,
                    width: NODE_W, height: nodeHeight(model),
                    borderRadius: 8, overflow: 'hidden', userSelect: 'none',
                    cursor: lockMode ? 'grab' : 'pointer',
                    border: `1px solid ${isSel ? color : hasSelEdge ? color + '55' : 'var(--node-border)'}`,
                    boxShadow: isSel
                      ? `0 0 0 2.5px ${color}38, 0 8px 28px rgba(0,0,0,0.32)`
                      : hasSelEdge ? `0 0 0 1.5px ${color}22, var(--node-shadow)` : 'var(--node-shadow)',
                    background: 'var(--node-bg)',
                    transition: 'border-color 0.12s, box-shadow 0.12s',
                  }}
                >
                  {/* ── Accent bar ── */}
                  <div style={{ height: 3, background: color, flexShrink: 0 }} />

                  {/* ── Header ── */}
                  <div style={{
                    height: HEADER_H - 3,
                    display: 'flex', alignItems: 'center',
                    padding: '0 10px', gap: 7,
                    background: `${color}10`,
                    borderBottom: '1px solid var(--node-field-divider)',
                  }}>
                    {/* Table icon */}
                    <svg width="12" height="12" viewBox="0 0 16 16" fill="none"
                      style={{ flexShrink: 0, color, opacity: 0.8 }}>
                      <rect x="1.5" y="1.5" width="13" height="13" rx="2"
                        stroke="currentColor" strokeWidth="1.5"/>
                      <line x1="1.5" y1="6"    x2="14.5" y2="6"    stroke="currentColor" strokeWidth="1"/>
                      <line x1="1.5" y1="10.5" x2="14.5" y2="10.5" stroke="currentColor" strokeWidth="1"/>
                      <line x1="6"   y1="6"    x2="6"    y2="14.5" stroke="currentColor" strokeWidth="1"/>
                    </svg>
                    <span style={{
                      fontWeight: 700, fontSize: 12, flex: 1,
                      overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                      color: 'var(--node-text)',
                    }}>
                      {model.name}
                    </span>
                    <span style={{ fontSize: 9, flexShrink: 0, color: 'var(--node-text-dim)' }}>
                      {model.fields.length}
                    </span>
                  </div>

                  {/* ── Fields ── */}
                  {model.fields.map((f, fi) => (
                    <div key={f.name} style={{
                      height: FIELD_H,
                      display: 'flex', alignItems: 'center',
                      padding: '0 10px', gap: 6,
                      borderBottom: fi < model.fields.length - 1
                        ? '1px solid var(--node-field-divider)' : 'none',
                    }}>
                      {/* Left icon: key for PK, filled dot for FK/relation, hollow dot for scalar */}
                      {f.isPrimary ? (
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none"
                          style={{ flexShrink: 0, color: '#f59e0b' }}>
                          <circle cx="7.5" cy="15.5" r="5" stroke="currentColor" strokeWidth="2"/>
                          <path d="M21.5 2.5L13 11m0 0l2.5 2.5M13 11l-2.5 2.5"
                            stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
                        </svg>
                      ) : (
                        <div style={{
                          width: 7, height: 7, borderRadius: '50%', flexShrink: 0,
                          background: f.isRelation ? color : 'transparent',
                          border: `1.5px solid ${f.isRelation ? 'transparent' : 'rgba(150,155,185,0.28)'}`,
                        }} />
                      )}

                      {/* Field name */}
                      <span style={{
                        fontSize: 11, flex: 1,
                        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                        fontWeight: f.isPrimary ? 600 : 400,
                        color: 'var(--node-text)',
                      }}>
                        {f.name}
                      </span>

                      {/* Type label */}
                      <span style={{
                        fontSize: 10, flexShrink: 0,
                        fontFamily: 'var(--vscode-editor-font-family, monospace)',
                        color: f.isPrimary ? '#f59e0b'
                             : f.isRelation ? color
                             : 'var(--node-text-dim)',
                      }}>
                        {f.type}
                      </span>
                    </div>
                  ))}
                </div>
              );
            })}
          </div>

          {/* ── Snap dot overlay — rendered above node divs ── */}
          {draggingEndpoint && snapPts && (
            <div style={{
              position: 'absolute', top: 0, left: 0,
              transformOrigin: '0 0',
              transform: `translate(${pan.x}px,${pan.y}px) scale(${scale})`,
              pointerEvents: 'none',
            }}>
              {snapPts
                .filter((pt) => {
                  const mpos = positions[pt.model];
                  if (!mpos) return false;
                  // Show only the side of each model that faces the cursor
                  const isLeft = pt.x === mpos.x;
                  const cursorLeft = draggingEndpoint.x < mpos.x + NODE_W / 2;
                  return isLeft === cursorLeft;
                })
                .map((pt, i) => {
                  const isNearest = nearestSnap !== null &&
                    pt.x === nearestSnap.x && pt.y === nearestSnap.y;
                  const r = isNearest ? 7 : 4;
                  return (
                    <div key={i} style={{
                      position: 'absolute',
                      left: pt.x - r, top: pt.y - r,
                      width: r * 2, height: r * 2,
                      borderRadius: '50%',
                      background: isNearest ? '#818cf8' : 'var(--node-bg)',
                      border: `${isNearest ? 2 : 1.5}px solid ${isNearest ? '#818cf8' : 'rgba(129,140,248,0.65)'}`,
                      boxShadow: isNearest ? '0 0 0 4px rgba(129,140,248,0.22)' : 'none',
                      transition: 'all 0.08s ease',
                    }} />
                  );
                })}
            </div>
          )}

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

            {/* Layout button + popover */}
            <div style={{ position: 'relative' }}>
              <button
                style={navBtn(showLayoutMenu)}
                title="레이아웃 정렬"
                onClick={(e) => { e.stopPropagation(); setShowLayoutMenu((v) => !v); }}
              >
                <IconLayout />
              </button>
              {showLayoutMenu && (
                <div
                  onClick={(e) => e.stopPropagation()}
                  style={{
                    position: 'absolute', bottom: 'calc(100% + 8px)', left: '50%',
                    transform: 'translateX(-50%)',
                    background: 'var(--navbar-bg)',
                    border: '1px solid var(--navbar-border)',
                    borderRadius: 7,
                    boxShadow: '0 4px 16px rgba(0,0,0,0.3)',
                    overflow: 'hidden',
                    minWidth: 110,
                    zIndex: 20,
                  }}
                >
                  {([
                    { type: 'lr',        label: 'Left-right' },
                    { type: 'snowflake', label: 'Snowflake'  },
                    { type: 'compact',   label: 'Compact'    },
                  ] as { type: LayoutType; label: string }[]).map(({ type, label }) => (
                    <button
                      key={type}
                      onClick={() => applyLayout(type)}
                      style={{
                        display: 'block', width: '100%',
                        padding: '7px 14px', border: 'none',
                        background: 'transparent',
                        color: 'var(--vscode-foreground)',
                        fontSize: 11, textAlign: 'left', cursor: 'pointer',
                        borderBottom: type !== 'compact'
                          ? '1px solid var(--navbar-border)' : 'none',
                      }}
                      onMouseEnter={(e) => {
                        (e.currentTarget as HTMLButtonElement).style.background = 'rgba(99,102,241,0.15)';
                      }}
                      onMouseLeave={(e) => {
                        (e.currentTarget as HTMLButtonElement).style.background = 'transparent';
                      }}
                    >{label}</button>
                  ))}
                </div>
              )}
            </div>

            <div style={navDivider} />

            <button
              style={navBtn(lockMode)}
              title={lockMode ? '이동 모드 (클릭하여 편집 모드로)' : '편집 모드 (클릭하여 이동 모드로)'}
              onClick={() => { setLockMode((v) => !v); if (lockMode) { setSelected(null); setSelectedNodes(new Set()); } }}
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
                <div style={{
                  flex: 1, overflow: 'auto',
                  fontFamily: 'var(--vscode-editor-font-family, Consolas, monospace)',
                  fontSize: 11, lineHeight: '18px',
                  color: 'var(--diff-text, #d4d4d4)',
                  paddingBottom: 14,
                }}>
                  {JSON.stringify(modelToJson(selected, state.ormType), null, 2).split('\n').map((line, i) => (
                    <div key={i} style={{ display: 'flex', minHeight: 18 }}>
                      <span style={{
                        width: 32, flexShrink: 0, textAlign: 'right', paddingRight: 8,
                        color: 'var(--diff-linenum, rgba(255,255,255,0.25))',
                        userSelect: 'none', lineHeight: '18px',
                        borderRight: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.08))',
                      }}>{i + 1}</span>
                      <span style={{ paddingLeft: 10, whiteSpace: 'pre', lineHeight: '18px' }}>{line}</span>
                    </div>
                  ))}
                </div>

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
          {/* ORM type tabs */}
          <div style={{
            display: 'flex', gap: 4, padding: '5px 10px', flexShrink: 0, alignItems: 'center',
            borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.08))',
            background: 'var(--diff-header-bg, #2d2d2d)',
          }}>
            {ORM_TYPES.map((orm) => (
              <button key={orm}
                onClick={() => setState((p) => ({ ...p, ormType: orm, ormSource: DEFAULT_SCHEMAS[orm] }))}
                style={{
                  padding: '2px 8px', border: '1px solid', borderRadius: 3, fontSize: 10, cursor: 'pointer',
                  borderColor: state.ormType === orm ? 'var(--vscode-focusBorder, #007acc)' : 'var(--node-text)',
                  background:  state.ormType === orm ? 'var(--vscode-button-background, #0e639c)' : 'transparent',
                  color:       state.ormType === orm ? 'var(--vscode-button-foreground, #fff)' : 'var(--diff-header-text, #cccccc)',
                }}
              >{orm}</button>
            ))}
          </div>
          {/* Editor: line-number gutter + editable textarea */}
          <div style={{
            flex: 1, display: 'flex', overflow: 'hidden',
            background: 'var(--diff-bg, #1e1e1e)',
            fontFamily: 'var(--vscode-editor-font-family, Consolas, "Courier New", monospace)',
            fontSize: 12,
          }}>
            {/* Gutter — paddingTop must exactly match textarea's paddingTop */}
            <div
              id="code-drawer-gutter"
              style={{
                width: 44, paddingTop: 8, paddingBottom: 8, flexShrink: 0,
                textAlign: 'right', paddingRight: 10,
                color: 'var(--diff-linenum, rgba(255,255,255,0.25))',
                fontSize: 12, lineHeight: '20px',
                userSelect: 'none', overflowY: 'hidden',
                borderRight: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.08))',
                background: 'var(--diff-bg, #1e1e1e)',
              }}
            >
              {state.ormSource.split('\n').map((_, i) => (
                <div key={i} style={{ height: 20 }}>{i + 1}</div>
              ))}
            </div>
            {/* Textarea */}
            <textarea
              value={state.ormSource}
              onChange={(e) => setState((p) => ({ ...p, ormSource: e.target.value }))}
              onScroll={(e) => {
                const gutter = document.getElementById('code-drawer-gutter');
                if (gutter) gutter.scrollTop = e.currentTarget.scrollTop;
              }}
              spellCheck={false}
              style={{
                flex: 1, resize: 'none', border: 'none', outline: 'none',
                padding: '8px 12px',
                fontFamily: 'inherit', fontSize: 12, lineHeight: '20px',
                color: 'var(--diff-text, #d4d4d4)',
                background: 'var(--diff-bg, #1e1e1e)',
                overflowY: 'auto',
              }}
            />
          </div>
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
