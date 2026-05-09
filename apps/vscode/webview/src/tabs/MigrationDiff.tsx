import React, { useEffect, useRef, useState } from 'react';
import { postMessage } from '../vscode';
import type { AppState } from '../App';

// ── Types ─────────────────────────────────────────────────────────────────────

type Dialect  = 'postgres' | 'mysql' | 'sqlite';
type FileKind = 'create' | 'alter' | 'drop' | 'index';

interface SqlFile {
  id:      string;
  name:    string;
  kind:    FileKind;
  sql:     string;
  adds:    number;
  removes: number;
}

interface DiffLine {
  type: 'add' | 'remove' | 'ctx';
  num:  number;
  text: string;
}

// ── SQL parser ────────────────────────────────────────────────────────────────

function stripQuotes(s: string) {
  return s.replace(/^["`\[]/, '').replace(/["`\]]$/, '');
}

function extractName(stmt: string, keyword: string): string {
  const re = new RegExp(
    keyword + '\\s+(?:IF\\s+(?:NOT\\s+)?EXISTS\\s+)?([`"\\[]?\\w+[`"\\]]?)',
    'i'
  );
  return stripQuotes(stmt.match(re)?.[1] ?? 'unknown');
}

function parseSql(sql: string): SqlFile[] {
  if (!sql.trim()) return [];

  const stmts = sql
    .split(/;\s*(?:\n|$)/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => s + ';');

  const byKey = new Map<string, SqlFile>();
  let idx = 0;

  for (const stmt of stmts) {
    const up = stmt.trimStart().toUpperCase();
    let kind: FileKind;
    let name: string;

    if (up.startsWith('CREATE TABLE')) {
      kind = 'create'; name = extractName(stmt, 'CREATE TABLE');
    } else if (up.startsWith('ALTER TABLE')) {
      kind = 'alter';  name = extractName(stmt, 'ALTER TABLE');
    } else if (up.startsWith('DROP TABLE')) {
      kind = 'drop';   name = extractName(stmt, 'DROP TABLE');
    } else if (up.startsWith('CREATE') && up.includes('INDEX')) {
      kind = 'index';  name = extractName(stmt, 'ON');
    } else if (up.startsWith('DROP INDEX')) {
      kind = 'index';  name = extractName(stmt, 'DROP INDEX');
    } else {
      continue;
    }

    // Count changed lines
    const lines = stmt.split('\n');
    let adds = 0, removes = 0;
    if (kind === 'create' || kind === 'index') {
      adds = lines.filter((l) => l.trim()).length;
    } else if (kind === 'drop') {
      removes = lines.filter((l) => l.trim()).length;
    } else {
      for (const l of lines) {
        const t = l.trim().toUpperCase();
        if (t.startsWith('ADD')) adds++;
        else if (t.startsWith('DROP')) removes++;
      }
      if (!adds && !removes) adds = lines.filter((l) => l.trim()).length;
    }

    // Merge multiple statements for the same table
    const key = `${kind === 'index' ? 'idx' : name}`;
    if (byKey.has(key)) {
      const f = byKey.get(key)!;
      f.sql     += '\n\n' + stmt;
      f.adds    += adds;
      f.removes += removes;
    } else {
      byKey.set(key, { id: String(idx++), name, kind, sql: stmt, adds, removes });
    }
  }

  return Array.from(byKey.values());
}

function toDiffLines(file: SqlFile): DiffLine[] {
  let num = 1;
  return file.sql.split('\n').map((text) => {
    const t = text.trim().toUpperCase();
    let type: DiffLine['type'] = 'ctx';

    if (file.kind === 'create' || file.kind === 'index') {
      type = 'add';
    } else if (file.kind === 'drop') {
      type = 'remove';
    } else {
      // alter: classify per line
      if (t.startsWith('ADD') || t.startsWith('ADD COLUMN') || t.startsWith('CREATE INDEX')) {
        type = 'add';
      } else if (t.startsWith('DROP') || t.startsWith('DROP COLUMN')) {
        type = 'remove';
      }
    }

    return { type, num: num++, text };
  });
}

// ── Constants & helpers ───────────────────────────────────────────────────────

const DIALECTS: { id: Dialect; label: string }[] = [
  { id: 'postgres', label: 'PostgreSQL' },
  { id: 'mysql',    label: 'MySQL'      },
  { id: 'sqlite',   label: 'SQLite'     },
];

function kindBadge(kind: FileKind): { label: string; color: string } {
  if (kind === 'create') return { label: 'A', color: '#4ade80' };
  if (kind === 'drop')   return { label: 'D', color: '#f87171' };
  if (kind === 'index')  return { label: 'I', color: '#60a5fa' };
  return { label: 'M', color: '#fbbf24' };
}

// ── Component ─────────────────────────────────────────────────────────────────

type Props = { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> };

export default function MigrationDiff({ state, setState: _setState }: Props) {
  const lastSchemaRef = useRef<string>('');
  const [requested,  setRequested]  = useState(false);
  const [dialect,    setDialect]    = useState<Dialect>('postgres');
  const [selectedId, setSelectedId] = useState<string | null>(null);

  // Trigger on first mount
  useEffect(() => {
    if (!requested) {
      setRequested(true);
      postMessage({ type: 'generate_migration', schema: state.schema, db: 'postgres' });
      lastSchemaRef.current = JSON.stringify(state.schema);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Re-generate when schema changes
  useEffect(() => {
    if (!requested) return;
    const key = JSON.stringify(state.schema);
    if (key === lastSchemaRef.current) return;
    lastSchemaRef.current = key;
    postMessage({ type: 'generate_migration', schema: state.schema, db: 'postgres' });
  }, [state.schema, requested]);

  const sql =
    dialect === 'postgres' ? state.postgres
    : dialect === 'mysql'  ? state.mysql
    : state.sqlite;

  const files = parseSql(sql);

  // Auto-select first file when SQL changes
  useEffect(() => {
    if (files.length > 0 && (!selectedId || !files.find((f) => f.id === selectedId))) {
      setSelectedId(files[0].id);
    }
  }, [sql]); // eslint-disable-line react-hooks/exhaustive-deps

  const selected   = files.find((f) => f.id === selectedId) ?? files[0] ?? null;
  const diffLines  = selected ? toDiffLines(selected) : [];
  const empty      = !state.postgres && !state.mysql && !state.sqlite;

  const totalAdds    = files.reduce((s, f) => s + f.adds, 0);
  const totalRemoves = files.reduce((s, f) => s + f.removes, 0);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>

      {/* ── Dialect tab bar ── */}
      <div style={{
        display: 'flex', alignItems: 'center', flexShrink: 0,
        borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
        background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
        padding: '0 12px',
        gap: 0,
      }}>
        {DIALECTS.map((d) => (
          <button
            key={d.id}
            onClick={() => setDialect(d.id)}
            style={{
              padding: '7px 14px', border: 'none', cursor: 'pointer',
              borderBottom: dialect === d.id
                ? '2px solid var(--vscode-focusBorder, #007acc)'
                : '2px solid transparent',
              background: 'transparent',
              color: dialect === d.id
                ? 'var(--vscode-foreground)'
                : 'var(--vscode-tab-inactiveForeground, #8e8e8e)',
              fontSize: 11,
              fontWeight: dialect === d.id ? 600 : 400,
              transition: 'color 0.1s',
            }}
          >{d.label}</button>
        ))}
        <div style={{ flex: 1 }} />
        {!empty && (
          <span style={{ fontSize: 10, opacity: 0.4, display: 'flex', gap: 6 }}>
            <span>{files.length} files</span>
            {totalAdds    > 0 && <span style={{ color: '#4ade80' }}>+{totalAdds}</span>}
            {totalRemoves > 0 && <span style={{ color: '#f87171' }}>−{totalRemoves}</span>}
          </span>
        )}
      </div>

      {/* ── Empty state ── */}
      {empty && (
        <div style={{
          flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center',
          opacity: 0.35, fontSize: 12, textAlign: 'center', lineHeight: 1.8,
        }}>
          <div>
            <div>ORM Editor 탭에서 스키마를 입력하면</div>
            <div>마이그레이션 SQL이 여기에 표시됩니다</div>
          </div>
        </div>
      )}

      {/* ── Main layout: file list + diff viewer ── */}
      {!empty && (
        <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>

          {/* ── Left: file list ── */}
          <div style={{
            width: 220, flexShrink: 0, display: 'flex', flexDirection: 'column',
            borderRight: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            background: 'var(--vscode-sideBar-background, #252526)',
            overflow: 'hidden',
          }}>
            <div style={{
              padding: '8px 12px 5px',
              fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', opacity: 0.4,
              flexShrink: 0,
            }}>CHANGES</div>

            <div style={{ flex: 1, overflowY: 'auto' }}>
              {files.map((f) => {
                const isSel = selected?.id === f.id;
                const badge = kindBadge(f.kind);
                return (
                  <div
                    key={f.id}
                    onClick={() => setSelectedId(f.id)}
                    style={{
                      display: 'flex', alignItems: 'center', gap: 7,
                      padding: '5px 10px 5px 0', cursor: 'pointer',
                      background: isSel
                        ? 'rgba(99,102,241,0.15)'
                        : 'transparent',
                      borderLeft: isSel
                        ? '2px solid var(--vscode-focusBorder, #007acc)'
                        : '2px solid transparent',
                      paddingLeft: isSel ? 10 : 10,
                    }}
                  >
                    {/* Table/index icon */}
                    <span style={{ fontSize: 11, opacity: 0.35, flexShrink: 0, marginLeft: 10 }}>
                      {f.kind === 'index' ? '⊞' : '≡'}
                    </span>
                    {/* File name */}
                    <span style={{
                      flex: 1, fontSize: 12,
                      overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                    }}>
                      {f.name}
                      <span style={{ opacity: 0.35 }}>.sql</span>
                    </span>
                    {/* +/− counts */}
                    <span style={{ fontSize: 10, color: '#4ade80', flexShrink: 0, minWidth: f.adds > 0 ? undefined : 0 }}>
                      {f.adds > 0 ? `+${f.adds}` : ''}
                    </span>
                    <span style={{ fontSize: 10, color: '#f87171', flexShrink: 0, minWidth: f.removes > 0 ? undefined : 0 }}>
                      {f.removes > 0 ? `−${f.removes}` : ''}
                    </span>
                    {/* Kind badge */}
                    <span style={{
                      fontSize: 10, fontWeight: 700, color: badge.color,
                      width: 14, textAlign: 'right', flexShrink: 0, marginRight: 4,
                    }}>
                      {badge.label}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>

          {/* ── Right: diff viewer ── */}
          <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>

            {/* Diff file header */}
            {selected && (
              <div style={{
                display: 'flex', alignItems: 'center', gap: 8, padding: '5px 12px',
                flexShrink: 0, fontSize: 12,
                borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
                background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
              }}>
                <span style={{ opacity: 0.35 }}>
                  {selected.kind === 'index' ? '⊞' : '≡'}
                </span>
                <span style={{ fontWeight: 600 }}>{selected.name}</span>
                <span style={{ opacity: 0.35 }}>.sql</span>
                <div style={{ flex: 1 }} />
                {selected.adds    > 0 && <span style={{ fontSize: 11, color: '#4ade80' }}>+{selected.adds}</span>}
                {selected.removes > 0 && <span style={{ fontSize: 11, color: '#f87171' }}>−{selected.removes}</span>}
                <button
                  onClick={() => navigator.clipboard.writeText(selected.sql).catch(console.error)}
                  style={{
                    background: 'transparent', cursor: 'pointer',
                    border: '1px solid var(--vscode-input-border, rgba(255,255,255,0.2))',
                    borderRadius: 3, color: 'var(--vscode-foreground)',
                    padding: '1px 8px', fontSize: 10,
                  }}
                >복사</button>
              </div>
            )}

            {/* Diff lines */}
            <div style={{
              flex: 1, overflow: 'auto',
              background: 'var(--vscode-editor-background, #1e1e1e)',
              fontFamily: 'var(--vscode-editor-font-family, Consolas, "Courier New", monospace)',
              fontSize: 12, lineHeight: '20px',
            }}>
              {diffLines.map((line, i) => (
                <div
                  key={i}
                  style={{
                    display: 'flex', minHeight: 20,
                    background:
                      line.type === 'add'    ? 'rgba(74,222,128,0.08)' :
                      line.type === 'remove' ? 'rgba(248,113,113,0.08)' :
                      'transparent',
                    borderLeft:
                      line.type === 'add'    ? '3px solid rgba(74,222,128,0.45)' :
                      line.type === 'remove' ? '3px solid rgba(248,113,113,0.45)' :
                      '3px solid transparent',
                  }}
                >
                  {/* Line number */}
                  <span style={{
                    minWidth: 44, paddingRight: 10, textAlign: 'right', flexShrink: 0,
                    fontSize: 11, userSelect: 'none', lineHeight: '20px',
                    color: 'var(--vscode-editorLineNumber-foreground, rgba(255,255,255,0.2))',
                  }}>
                    {line.num}
                  </span>
                  {/* +/− gutter */}
                  <span style={{
                    width: 18, flexShrink: 0, textAlign: 'center', userSelect: 'none',
                    lineHeight: '20px', fontSize: 12,
                    color:
                      line.type === 'add'    ? '#4ade80' :
                      line.type === 'remove' ? '#f87171' :
                      'transparent',
                  }}>
                    {line.type === 'add' ? '+' : line.type === 'remove' ? '−' : ' '}
                  </span>
                  {/* Code */}
                  <span style={{
                    flex: 1, paddingRight: 16, lineHeight: '20px',
                    whiteSpace: 'pre',
                    color:
                      line.type === 'add'    ? '#bbf7d0' :
                      line.type === 'remove' ? '#fecaca' :
                      'var(--vscode-editor-foreground, #d4d4d4)',
                  }}>
                    {line.text}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
