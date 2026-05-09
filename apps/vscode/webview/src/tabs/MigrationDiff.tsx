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

// ── Pre-built dummy files (avoids runtime parse failure) ──────────────────────

const PG_USERS = `CREATE TABLE "users" (
  "id" SERIAL NOT NULL,
  "email" TEXT NOT NULL,
  "name" TEXT,
  "created_at" TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW(),
  CONSTRAINT "pk_users" PRIMARY KEY ("id"),
  CONSTRAINT "uq_users__email" UNIQUE ("email")
);`;

const PG_PROFILES = `CREATE TABLE "profiles" (
  "id" SERIAL NOT NULL,
  "bio" TEXT NOT NULL,
  "user_id" INTEGER NOT NULL,
  CONSTRAINT "pk_profiles" PRIMARY KEY ("id"),
  CONSTRAINT "fk_profiles__user_id" FOREIGN KEY ("user_id") REFERENCES "users" ("id"),
  CONSTRAINT "uq_profiles__user_id" UNIQUE ("user_id")
);`;

const PG_POSTS = `CREATE TABLE "posts" (
  "id" SERIAL NOT NULL,
  "title" TEXT NOT NULL,
  "content" TEXT,
  "published" BOOLEAN NOT NULL DEFAULT false,
  "created_at" TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW(),
  "author_id" INTEGER NOT NULL,
  CONSTRAINT "pk_posts" PRIMARY KEY ("id"),
  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")
);`;

const PG_TAGS = `CREATE TABLE "tags" (
  "id" SERIAL NOT NULL,
  "name" TEXT NOT NULL,
  CONSTRAINT "pk_tags" PRIMARY KEY ("id"),
  CONSTRAINT "uq_tags__name" UNIQUE ("name")
);`;

const PG_TAG_ON_POSTS = `CREATE TABLE "tag_on_posts" (
  "post_id" INTEGER NOT NULL,
  "tag_id" INTEGER NOT NULL,
  CONSTRAINT "pk_tag_on_posts" PRIMARY KEY ("post_id", "tag_id"),
  CONSTRAINT "fk_tag_on_posts__post_id" FOREIGN KEY ("post_id") REFERENCES "posts" ("id"),
  CONSTRAINT "fk_tag_on_posts__tag_id" FOREIGN KEY ("tag_id") REFERENCES "tags" ("id")
);`;

const PG_INDEXES = `CREATE INDEX "ix_posts__author_id" ON "posts" ("author_id");
CREATE INDEX "ix_tag_on_posts__tag_id" ON "tag_on_posts" ("tag_id");`;

const MY_USERS = `CREATE TABLE \`users\` (
  \`id\` INT NOT NULL AUTO_INCREMENT,
  \`email\` VARCHAR(191) NOT NULL,
  \`name\` VARCHAR(191),
  \`created_at\` DATETIME(3) DEFAULT CURRENT_TIMESTAMP(3),
  PRIMARY KEY (\`id\`),
  CONSTRAINT \`uq_users__email\` UNIQUE (\`email\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;`;

const MY_PROFILES = `CREATE TABLE \`profiles\` (
  \`id\` INT NOT NULL AUTO_INCREMENT,
  \`bio\` TEXT NOT NULL,
  \`user_id\` INT NOT NULL,
  PRIMARY KEY (\`id\`),
  CONSTRAINT \`fk_profiles__user_id\` FOREIGN KEY (\`user_id\`) REFERENCES \`users\` (\`id\`),
  CONSTRAINT \`uq_profiles__user_id\` UNIQUE (\`user_id\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;`;

const MY_POSTS = `CREATE TABLE \`posts\` (
  \`id\` INT NOT NULL AUTO_INCREMENT,
  \`title\` VARCHAR(191) NOT NULL,
  \`content\` TEXT,
  \`published\` TINYINT(1) NOT NULL DEFAULT 0,
  \`created_at\` DATETIME(3) DEFAULT CURRENT_TIMESTAMP(3),
  \`author_id\` INT NOT NULL,
  PRIMARY KEY (\`id\`),
  CONSTRAINT \`fk_posts__author_id\` FOREIGN KEY (\`author_id\`) REFERENCES \`users\` (\`id\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;`;

const MY_TAGS = `CREATE TABLE \`tags\` (
  \`id\` INT NOT NULL AUTO_INCREMENT,
  \`name\` VARCHAR(191) NOT NULL,
  PRIMARY KEY (\`id\`),
  CONSTRAINT \`uq_tags__name\` UNIQUE (\`name\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;`;

const MY_TAG_ON_POSTS = `CREATE TABLE \`tag_on_posts\` (
  \`post_id\` INT NOT NULL,
  \`tag_id\` INT NOT NULL,
  PRIMARY KEY (\`post_id\`, \`tag_id\`),
  CONSTRAINT \`fk_tag_on_posts__post_id\` FOREIGN KEY (\`post_id\`) REFERENCES \`posts\` (\`id\`),
  CONSTRAINT \`fk_tag_on_posts__tag_id\` FOREIGN KEY (\`tag_id\`) REFERENCES \`tags\` (\`id\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;`;

const MY_INDEXES = `CREATE INDEX \`ix_posts__author_id\` ON \`posts\` (\`author_id\`);
CREATE INDEX \`ix_tag_on_posts__tag_id\` ON \`tag_on_posts\` (\`tag_id\`);`;

const SQ_USERS = `CREATE TABLE "users" (
  "id" INTEGER NOT NULL,
  "email" TEXT NOT NULL,
  "name" TEXT,
  "created_at" TEXT DEFAULT (datetime('now')),
  CONSTRAINT "pk_users" PRIMARY KEY ("id" AUTOINCREMENT),
  CONSTRAINT "uq_users__email" UNIQUE ("email")
);`;

const SQ_PROFILES = `CREATE TABLE "profiles" (
  "id" INTEGER NOT NULL,
  "bio" TEXT NOT NULL,
  "user_id" INTEGER NOT NULL,
  CONSTRAINT "pk_profiles" PRIMARY KEY ("id" AUTOINCREMENT),
  CONSTRAINT "fk_profiles__user_id" FOREIGN KEY ("user_id") REFERENCES "users" ("id"),
  CONSTRAINT "uq_profiles__user_id" UNIQUE ("user_id")
);`;

const SQ_POSTS = `CREATE TABLE "posts" (
  "id" INTEGER NOT NULL,
  "title" TEXT NOT NULL,
  "content" TEXT,
  "published" INTEGER NOT NULL DEFAULT 0,
  "created_at" TEXT DEFAULT (datetime('now')),
  "author_id" INTEGER NOT NULL,
  CONSTRAINT "pk_posts" PRIMARY KEY ("id" AUTOINCREMENT),
  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")
);`;

const SQ_TAGS = `CREATE TABLE "tags" (
  "id" INTEGER NOT NULL,
  "name" TEXT NOT NULL,
  CONSTRAINT "pk_tags" PRIMARY KEY ("id" AUTOINCREMENT),
  CONSTRAINT "uq_tags__name" UNIQUE ("name")
);`;

const SQ_TAG_ON_POSTS = `CREATE TABLE "tag_on_posts" (
  "post_id" INTEGER NOT NULL,
  "tag_id" INTEGER NOT NULL,
  CONSTRAINT "pk_tag_on_posts" PRIMARY KEY ("post_id", "tag_id"),
  CONSTRAINT "fk_tag_on_posts__post_id" FOREIGN KEY ("post_id") REFERENCES "posts" ("id"),
  CONSTRAINT "fk_tag_on_posts__tag_id" FOREIGN KEY ("tag_id") REFERENCES "tags" ("id")
);`;

const SQ_INDEXES = `CREATE INDEX "ix_posts__author_id" ON "posts" ("author_id");
CREATE INDEX "ix_tag_on_posts__tag_id" ON "tag_on_posts" ("tag_id");`;

function makeFile(id: string, name: string, kind: FileKind, sql: string): SqlFile {
  const lines = sql.split('\n').filter((l) => l.trim());
  const adds    = kind === 'create' || kind === 'index' ? lines.length : lines.filter(l => /^\s+ADD/i.test(l)).length;
  const removes = kind === 'drop' ? lines.length : lines.filter(l => /^\s+DROP/i.test(l)).length;
  return { id, name, kind, sql, adds, removes };
}

const DUMMY_FILES: Record<Dialect, SqlFile[]> = {
  postgres: [
    makeFile('0', 'users',        'create', PG_USERS),
    makeFile('1', 'profiles',     'create', PG_PROFILES),
    makeFile('2', 'posts',        'create', PG_POSTS),
    makeFile('3', 'tags',         'create', PG_TAGS),
    makeFile('4', 'tag_on_posts', 'create', PG_TAG_ON_POSTS),
    makeFile('5', 'indexes',      'index',  PG_INDEXES),
  ],
  mysql: [
    makeFile('0', 'users',        'create', MY_USERS),
    makeFile('1', 'profiles',     'create', MY_PROFILES),
    makeFile('2', 'posts',        'create', MY_POSTS),
    makeFile('3', 'tags',         'create', MY_TAGS),
    makeFile('4', 'tag_on_posts', 'create', MY_TAG_ON_POSTS),
    makeFile('5', 'indexes',      'index',  MY_INDEXES),
  ],
  sqlite: [
    makeFile('0', 'users',        'create', SQ_USERS),
    makeFile('1', 'profiles',     'create', SQ_PROFILES),
    makeFile('2', 'posts',        'create', SQ_POSTS),
    makeFile('3', 'tags',         'create', SQ_TAGS),
    makeFile('4', 'tag_on_posts', 'create', SQ_TAG_ON_POSTS),
    makeFile('5', 'indexes',      'index',  SQ_INDEXES),
  ],
};

// ── SQL parser (for real WASM output) ────────────────────────────────────────

function stripQuotes(s: string) {
  return s.replace(/^["`[\u0060]/, '').replace(/["`\]]$/, '');
}

function extractName(stmt: string, keyword: string): string {
  const re = new RegExp(
    keyword + '\\s+(?:IF\\s+(?:NOT\\s+)?EXISTS\\s+)?([`"[\\[]?\\w+[`"\\]]?)',
    'i'
  );
  return stripQuotes(stmt.match(re)?.[1] ?? 'unknown');
}

function parseSql(sql: string): SqlFile[] {
  if (!sql.trim()) return [];
  const stmts = sql
    .split(/;[ \t]*\n/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => (s.endsWith(';') ? s : s + ';'));

  const byKey = new Map<string, SqlFile>();
  let idx = 0;

  for (const stmt of stmts) {
    const up = stmt.trimStart().toUpperCase();
    let kind: FileKind;
    let name: string;

    if (up.startsWith('CREATE TABLE'))         { kind = 'create'; name = extractName(stmt, 'CREATE TABLE'); }
    else if (up.startsWith('ALTER TABLE'))      { kind = 'alter';  name = extractName(stmt, 'ALTER TABLE'); }
    else if (up.startsWith('DROP TABLE'))       { kind = 'drop';   name = extractName(stmt, 'DROP TABLE'); }
    else if (up.includes('INDEX'))              { kind = 'index';  name = 'indexes'; }
    else continue;

    const lines = stmt.split('\n');
    let adds = 0, removes = 0;
    if (kind === 'create' || kind === 'index') { adds = lines.filter(l => l.trim()).length; }
    else if (kind === 'drop')                  { removes = lines.filter(l => l.trim()).length; }
    else {
      lines.forEach(l => { if (/^\s+ADD/i.test(l)) adds++; else if (/^\s+DROP/i.test(l)) removes++; });
      if (!adds && !removes) adds = lines.filter(l => l.trim()).length;
    }

    const key = name;
    if (byKey.has(key)) {
      const f = byKey.get(key)!;
      f.sql += '\n\n' + stmt;
      f.adds += adds;
      f.removes += removes;
    } else {
      byKey.set(key, { id: String(idx++), name, kind, sql: stmt, adds, removes });
    }
  }
  return Array.from(byKey.values());
}

// ── Diff line generator ───────────────────────────────────────────────────────

function toDiffLines(file: SqlFile): DiffLine[] {
  let num = 1;
  return file.sql.split('\n').map((text) => {
    let type: DiffLine['type'] = 'ctx';
    if (file.kind === 'create' || file.kind === 'index') {
      type = 'add';
    } else if (file.kind === 'drop') {
      type = 'remove';
    } else {
      const t = text.trim().toUpperCase();
      if (t.startsWith('ADD'))  type = 'add';
      else if (t.startsWith('DROP')) type = 'remove';
    }
    return { type, num: num++, text };
  });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const DIALECT_LABELS: Record<Dialect, string> = { postgres: 'PG', mysql: 'MY', sqlite: 'SQ' };

function kindBadge(kind: FileKind) {
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
  const [selectedId, setSelectedId] = useState<string>('0');

  useEffect(() => {
    if (!requested) {
      setRequested(true);
      postMessage({ type: 'generate_migration', schema: state.schema, db: 'postgres' });
      lastSchemaRef.current = JSON.stringify(state.schema);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!requested) return;
    const key = JSON.stringify(state.schema);
    if (key === lastSchemaRef.current) return;
    lastSchemaRef.current = key;
    postMessage({ type: 'generate_migration', schema: state.schema, db: 'postgres' });
  }, [state.schema, requested]);

  // Real SQL from WASM; fall back to pre-built dummy files
  const realSql =
    dialect === 'postgres' ? state.postgres :
    dialect === 'mysql'    ? state.mysql    : state.sqlite;

  const hasReal = !!realSql;
  const files   = hasReal ? parseSql(realSql) : DUMMY_FILES[dialect];

  // Reset selection to first file when dialect changes
  useEffect(() => { setSelectedId('0'); }, [dialect]);

  const selected  = files.find((f) => f.id === selectedId) ?? files[0];
  const diffLines = selected ? toDiffLines(selected) : [];

  const totalAdds    = files.reduce((s, f) => s + f.adds,    0);
  const totalRemoves = files.reduce((s, f) => s + f.removes, 0);

  return (
    <div style={{ display: 'flex', height: '100%', overflow: 'hidden' }}>

      {/* ── Left: file list ── */}
      <div style={{
        width: 224, flexShrink: 0, display: 'flex', flexDirection: 'column',
        borderRight: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
        background: 'var(--vscode-sideBar-background, #252526)',
      }}>
        {/* Header */}
        <div style={{
          padding: '8px 10px 6px', flexShrink: 0, display: 'flex', alignItems: 'center',
          justifyContent: 'space-between',
          borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.08))',
        }}>
          <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', opacity: 0.4 }}>
            CHANGES
          </span>
          {/* Dialect toggle */}
          <div style={{ display: 'flex', gap: 1, padding: 2, background: 'rgba(0,0,0,0.25)', borderRadius: 5 }}>
            {(['postgres', 'mysql', 'sqlite'] as Dialect[]).map((d) => (
              <button key={d} onClick={() => setDialect(d)} style={{
                padding: '2px 7px', border: 'none', borderRadius: 4, cursor: 'pointer',
                fontSize: 10, fontWeight: 600,
                background: dialect === d ? 'var(--vscode-button-background, #0e639c)' : 'transparent',
                color:      dialect === d ? '#fff' : 'var(--vscode-tab-inactiveForeground, #888)',
                transition: 'background 0.1s',
              }}>{DIALECT_LABELS[d]}</button>
            ))}
          </div>
        </div>

        {/* Summary */}
        <div style={{ padding: '4px 10px', fontSize: 10, display: 'flex', gap: 6, alignItems: 'center', opacity: 0.5, flexShrink: 0 }}>
          <span>{files.length} files</span>
          {totalAdds    > 0 && <span style={{ color: '#4ade80' }}>+{totalAdds}</span>}
          {totalRemoves > 0 && <span style={{ color: '#f87171' }}>−{totalRemoves}</span>}
          {!hasReal && (
            <span style={{
              marginLeft: 'auto', fontSize: 9, padding: '1px 5px', borderRadius: 3,
              background: 'rgba(251,191,36,0.15)', color: '#fbbf24',
              border: '1px solid rgba(251,191,36,0.28)',
            }}>PREVIEW</span>
          )}
        </div>

        {/* File list */}
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {files.map((f) => {
            const isSel = f.id === selectedId;
            const badge = kindBadge(f.kind);
            return (
              <div key={f.id} onClick={() => setSelectedId(f.id)} style={{
                display: 'flex', alignItems: 'center', gap: 6,
                padding: '5px 10px', cursor: 'pointer',
                background: isSel ? 'rgba(99,102,241,0.15)' : 'transparent',
                borderLeft: isSel ? '2px solid var(--vscode-focusBorder, #007acc)' : '2px solid transparent',
              }}>
                <span style={{ fontSize: 11, opacity: 0.3, flexShrink: 0 }}>
                  {f.kind === 'index' ? '⊞' : '≡'}
                </span>
                <span style={{ flex: 1, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {f.name}<span style={{ opacity: 0.35 }}>.sql</span>
                </span>
                {f.adds    > 0 && <span style={{ fontSize: 10, color: '#4ade80', flexShrink: 0 }}>+{f.adds}</span>}
                {f.removes > 0 && <span style={{ fontSize: 10, color: '#f87171', flexShrink: 0 }}>−{f.removes}</span>}
                <span style={{ fontSize: 10, fontWeight: 700, color: badge.color, width: 12, textAlign: 'right', flexShrink: 0 }}>
                  {badge.label}
                </span>
              </div>
            );
          })}
        </div>
      </div>

      {/* ── Right: diff viewer ── */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>

        {/* File header */}
        {selected && (
          <div style={{
            display: 'flex', alignItems: 'center', gap: 8, padding: '5px 12px', flexShrink: 0,
            borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
            fontSize: 12,
          }}>
            <span style={{ opacity: 0.35 }}>{selected.kind === 'index' ? '⊞' : '≡'}</span>
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
            <div key={i} style={{
              display: 'flex', minHeight: 20,
              background:
                line.type === 'add'    ? 'rgba(74,222,128,0.08)'  :
                line.type === 'remove' ? 'rgba(248,113,113,0.08)' : 'transparent',
              borderLeft:
                line.type === 'add'    ? '3px solid rgba(74,222,128,0.45)'  :
                line.type === 'remove' ? '3px solid rgba(248,113,113,0.45)' : '3px solid transparent',
            }}>
              <span style={{
                minWidth: 44, paddingRight: 10, textAlign: 'right', flexShrink: 0,
                fontSize: 11, lineHeight: '20px', userSelect: 'none',
                color: 'var(--vscode-editorLineNumber-foreground, rgba(255,255,255,0.2))',
              }}>{line.num}</span>
              <span style={{
                width: 18, flexShrink: 0, textAlign: 'center', lineHeight: '20px',
                fontSize: 12, userSelect: 'none',
                color: line.type === 'add' ? '#4ade80' : line.type === 'remove' ? '#f87171' : 'transparent',
              }}>
                {line.type === 'add' ? '+' : line.type === 'remove' ? '−' : ' '}
              </span>
              <span style={{
                flex: 1, paddingRight: 16, lineHeight: '20px', whiteSpace: 'pre',
                color:
                  line.type === 'add'    ? '#bbf7d0' :
                  line.type === 'remove' ? '#fecaca' :
                  'var(--vscode-editor-foreground, #d4d4d4)',
              }}>{line.text}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
