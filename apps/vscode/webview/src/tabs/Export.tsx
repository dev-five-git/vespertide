import React, { useState } from 'react';
import { postMessage } from '../vscode';
import type { AppState } from '../App';
import { DEFAULT_SCHEMAS } from '../App';

// ── Types ─────────────────────────────────────────────────────────────────────

type ExportFile = {
  id:       string;
  label:    string;       // display name in file list
  ext:      string;       // e.g. ".sql", ".prisma"
  lang:     string;       // label for header badge
  content:  string;       // preview content
  isDummy:  boolean;
  onSave?:  () => void;
};

// ── Dummy export previews ─────────────────────────────────────────────────────

const DUMMY_ORM_JSON = `{
  "orm": "prisma",
  "tables": [
    {
      "name": "users",
      "columns": [
        { "name": "id",         "type": "integer",   "primary_key": { "auto_increment": true } },
        { "name": "email",      "type": "varchar",   "nullable": false },
        { "name": "name",       "type": "varchar",   "nullable": true  },
        { "name": "created_at", "type": "timestamp", "nullable": true  }
      ]
    },
    {
      "name": "posts",
      "columns": [
        { "name": "id",         "type": "integer",   "primary_key": { "auto_increment": true } },
        { "name": "title",      "type": "varchar",   "nullable": false },
        { "name": "content",    "type": "varchar",   "nullable": true  },
        { "name": "published",  "type": "boolean",   "nullable": false },
        { "name": "author_id",  "type": "integer",   "nullable": false }
      ]
    }
  ]
}`;

// ── Component ─────────────────────────────────────────────────────────────────

type Props = { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> };

export default function Export({ state }: Props) {
  const [selectedId, setSelectedId] = useState<string>('sql-pg');
  const [copied,     setCopied]     = useState<string | null>(null);
  const [saving,     setSaving]     = useState<string | null>(null);

  const hasSql    = !!(state.postgres || state.mysql || state.sqlite);
  const hasSvg    = !!state.svg;
  const hasSchema = Object.keys(state.schema).length > 0;

  const ormLabel = state.ormType.charAt(0).toUpperCase() + state.ormType.slice(1);

  const files: ExportFile[] = [
    // ── Migration SQL ──
    {
      id:      'sql-pg',
      label:   'migration.postgres',
      ext:     '.sql',
      lang:    'SQL',
      content: state.postgres || `-- PostgreSQL migration (preview)\n\nCREATE TABLE "users" (\n  "id" SERIAL NOT NULL,\n  "email" TEXT NOT NULL,\n  "name" TEXT,\n  "created_at" TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW(),\n  CONSTRAINT "pk_users" PRIMARY KEY ("id"),\n  CONSTRAINT "uq_users__email" UNIQUE ("email")\n);\n\nCREATE TABLE "posts" (\n  "id" SERIAL NOT NULL,\n  "title" TEXT NOT NULL,\n  "content" TEXT,\n  "published" BOOLEAN NOT NULL DEFAULT false,\n  "author_id" INTEGER NOT NULL,\n  CONSTRAINT "pk_posts" PRIMARY KEY ("id"),\n  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")\n);\n\nCREATE INDEX "ix_posts__author_id" ON "posts" ("author_id");`,
      isDummy: !hasSql,
      onSave:  () => { setSaving('sql-pg'); postMessage({ type: 'export_svg' }); setTimeout(() => setSaving(null), 2000); },
    },
    {
      id:      'sql-my',
      label:   'migration.mysql',
      ext:     '.sql',
      lang:    'SQL',
      content: state.mysql || `-- MySQL migration (preview)\n\nCREATE TABLE \`users\` (\n  \`id\` INT NOT NULL AUTO_INCREMENT,\n  \`email\` VARCHAR(191) NOT NULL,\n  \`name\` VARCHAR(191),\n  \`created_at\` DATETIME(3) DEFAULT CURRENT_TIMESTAMP(3),\n  PRIMARY KEY (\`id\`),\n  CONSTRAINT \`uq_users__email\` UNIQUE (\`email\`)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;\n\nCREATE TABLE \`posts\` (\n  \`id\` INT NOT NULL AUTO_INCREMENT,\n  \`title\` VARCHAR(191) NOT NULL,\n  \`content\` TEXT,\n  \`published\` TINYINT(1) NOT NULL DEFAULT 0,\n  \`author_id\` INT NOT NULL,\n  PRIMARY KEY (\`id\`),\n  CONSTRAINT \`fk_posts__author_id\` FOREIGN KEY (\`author_id\`) REFERENCES \`users\` (\`id\`)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;`,
      isDummy: !hasSql,
      onSave:  () => { setSaving('sql-my'); setTimeout(() => setSaving(null), 2000); },
    },
    {
      id:      'sql-sq',
      label:   'migration.sqlite',
      ext:     '.sql',
      lang:    'SQL',
      content: state.sqlite || `-- SQLite migration (preview)\n\nCREATE TABLE "users" (\n  "id" INTEGER NOT NULL,\n  "email" TEXT NOT NULL,\n  "name" TEXT,\n  "created_at" TEXT DEFAULT (datetime('now')),\n  CONSTRAINT "pk_users" PRIMARY KEY ("id" AUTOINCREMENT),\n  CONSTRAINT "uq_users__email" UNIQUE ("email")\n);\n\nCREATE TABLE "posts" (\n  "id" INTEGER NOT NULL,\n  "title" TEXT NOT NULL,\n  "content" TEXT,\n  "published" INTEGER NOT NULL DEFAULT 0,\n  "author_id" INTEGER NOT NULL,\n  CONSTRAINT "pk_posts" PRIMARY KEY ("id" AUTOINCREMENT),\n  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")\n);`,
      isDummy: !hasSql,
      onSave:  () => { setSaving('sql-sq'); setTimeout(() => setSaving(null), 2000); },
    },

    // ── ORM source ──
    {
      id:      'orm-src',
      label:   `schema.${state.ormType}`,
      ext:     state.ormType === 'prisma' ? '.prisma' : state.ormType === 'drizzle' ? '.ts' : state.ormType === 'gorm' ? '.go' : '.java',
      lang:    ormLabel,
      content: state.ormSource || DEFAULT_SCHEMAS[state.ormType],
      isDummy: !state.ormSource,
      onSave:  () => { setSaving('orm-src'); setTimeout(() => setSaving(null), 2000); },
    },

    // ── ERD SVG ──
    {
      id:      'erd-svg',
      label:   'erd-diagram',
      ext:     '.svg',
      lang:    'SVG',
      content: state.svg || `<!-- ERD Diagram (preview) -->\n<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200" viewBox="0 0 400 200">\n  <rect x="20" y="20" width="160" height="80" rx="8"\n    fill="#1e1e2e" stroke="#6366f1" stroke-width="1.5"/>\n  <text x="100" y="65" font-family="sans-serif" font-size="13"\n    fill="#a5b4fc" text-anchor="middle">users</text>\n\n  <rect x="220" y="20" width="160" height="80" rx="8"\n    fill="#1e1e2e" stroke="#8b5cf6" stroke-width="1.5"/>\n  <text x="300" y="65" font-family="sans-serif" font-size="13"\n    fill="#c4b5fd" text-anchor="middle">posts</text>\n\n  <path d="M180 60 C200 60, 200 60, 220 60"\n    fill="none" stroke="rgba(99,102,241,0.6)" stroke-width="1.5"\n    stroke-dasharray="5 3" marker-end="url(#a)"/>\n</svg>`,
      isDummy: !hasSvg,
      onSave:  () => { setSaving('erd-svg'); postMessage({ type: 'export_svg' }); setTimeout(() => setSaving(null), 2000); },
    },

    // ── PDF ──
    {
      id:      'erd-pdf',
      label:   'erd-diagram',
      ext:     '.pdf',
      lang:    'PDF',
      content: `PDF export converts the ERD diagram SVG to a portable document.\n\nClick "Save" to generate the PDF file.\n\n${hasSvg ? '✓ ERD is ready for export.' : '⚠ Open ORM Editor and enter a schema first.'}`,
      isDummy: !hasSvg,
      onSave:  () => { setSaving('erd-pdf'); postMessage({ type: 'export_pdf' }); setTimeout(() => setSaving(null), 2000); },
    },

    // ── JSON Schema ──
    {
      id:      'schema-json',
      label:   'schema',
      ext:     '.json',
      lang:    'JSON',
      content: hasSchema ? JSON.stringify(state.schema, null, 2) : DUMMY_ORM_JSON,
      isDummy: !hasSchema,
      onSave:  () => { setSaving('schema-json'); postMessage({ type: 'export_mcp', schema: state.schema }); setTimeout(() => setSaving(null), 2000); },
    },
  ];

  const selected = files.find((f) => f.id === selectedId) ?? files[0];

  const copy = (content: string, id: string) => {
    navigator.clipboard.writeText(content).then(() => {
      setCopied(id);
      setTimeout(() => setCopied(null), 1500);
    }).catch(console.error);
  };

  return (
    <div style={{ display: 'flex', height: '100%', overflow: 'hidden' }}>

      {/* ── Left: file list ── */}
      <div style={{
        width: 224, flexShrink: 0, display: 'flex', flexDirection: 'column',
        borderRight: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
        background: 'var(--vscode-sideBar-background, #252526)',
      }}>
        <div style={{
          padding: '8px 10px 6px', flexShrink: 0,
          borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.08))',
        }}>
          <span style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', opacity: 0.4 }}>
            EXPORT FILES
          </span>
        </div>

        {/* Group: Migration SQL */}
        <div style={{ padding: '8px 10px 3px', fontSize: 9, fontWeight: 600, opacity: 0.3, letterSpacing: '0.06em' }}>
          MIGRATION SQL
        </div>
        {files.filter(f => f.id.startsWith('sql-')).map(f => <FileRow key={f.id} f={f} selected={selected} onClick={() => setSelectedId(f.id)} />)}

        {/* Group: Schema */}
        <div style={{ padding: '10px 10px 3px', fontSize: 9, fontWeight: 600, opacity: 0.3, letterSpacing: '0.06em' }}>
          SCHEMA
        </div>
        {files.filter(f => f.id === 'orm-src' || f.id === 'schema-json').map(f => <FileRow key={f.id} f={f} selected={selected} onClick={() => setSelectedId(f.id)} />)}

        {/* Group: ERD */}
        <div style={{ padding: '10px 10px 3px', fontSize: 9, fontWeight: 600, opacity: 0.3, letterSpacing: '0.06em' }}>
          DIAGRAM
        </div>
        {files.filter(f => f.id.startsWith('erd-')).map(f => <FileRow key={f.id} f={f} selected={selected} onClick={() => setSelectedId(f.id)} />)}
      </div>

      {/* ── Right: preview ── */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>

        {/* File header */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8, padding: '5px 12px', flexShrink: 0,
          borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
          background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
          fontSize: 12,
        }}>
          <span style={{ fontWeight: 600 }}>{selected.label}</span>
          <span style={{ opacity: 0.35 }}>{selected.ext}</span>
          {/* Lang badge */}
          <span style={{
            fontSize: 9, padding: '1px 6px', borderRadius: 3,
            background: 'rgba(99,102,241,0.15)', color: '#a5b4fc',
            border: '1px solid rgba(99,102,241,0.25)', fontWeight: 700,
          }}>{selected.lang}</span>
          {selected.isDummy && (
            <span style={{
              fontSize: 9, padding: '1px 6px', borderRadius: 3,
              background: 'rgba(251,191,36,0.12)', color: '#fbbf24',
              border: '1px solid rgba(251,191,36,0.25)',
            }}>PREVIEW</span>
          )}
          <div style={{ flex: 1 }} />
          {/* Copy */}
          <button
            onClick={() => copy(selected.content, selected.id)}
            style={{
              background: copied === selected.id ? 'rgba(74,222,128,0.12)' : 'transparent',
              border: '1px solid var(--vscode-input-border, rgba(255,255,255,0.2))',
              borderRadius: 3, color: copied === selected.id ? '#4ade80' : 'var(--vscode-foreground)',
              padding: '2px 10px', fontSize: 10, cursor: 'pointer', transition: 'all 0.15s',
            }}
          >{copied === selected.id ? '✓ 복사됨' : '복사'}</button>
          {/* Save */}
          {selected.onSave && (
            <button
              onClick={selected.onSave}
              disabled={saving === selected.id}
              style={{
                background: saving === selected.id
                  ? 'rgba(99,102,241,0.2)'
                  : 'var(--vscode-button-background, #0e639c)',
                border: 'none', borderRadius: 3,
                color: 'var(--vscode-button-foreground, #fff)',
                padding: '2px 12px', fontSize: 10, cursor: saving === selected.id ? 'default' : 'pointer',
              }}
            >{saving === selected.id ? '저장 중...' : '저장'}</button>
          )}
        </div>

        {/* Preview body */}
        <div style={{
          flex: 1, overflow: 'auto',
          background: 'var(--vscode-editor-background, #1e1e1e)',
          fontFamily: selected.id === 'erd-svg'
            ? 'inherit'
            : 'var(--vscode-editor-font-family, Consolas, "Courier New", monospace)',
          fontSize: 12, lineHeight: '20px',
        }}>
          {selected.id === 'erd-svg' && selected.content.startsWith('<svg') ? (
            // Render SVG inline for preview
            <div style={{ padding: 24 }}>
              <div
                dangerouslySetInnerHTML={{ __html: selected.content }}
                style={{ maxWidth: '100%', overflow: 'auto' }}
              />
              <div style={{ marginTop: 16, fontSize: 11, opacity: 0.4 }}>
                SVG 소스:
              </div>
              <pre style={{
                marginTop: 8, padding: '10px 14px',
                background: 'rgba(0,0,0,0.2)', borderRadius: 6,
                fontSize: 11, lineHeight: 1.6, whiteSpace: 'pre', overflowX: 'auto',
                color: 'var(--vscode-editor-foreground, #d4d4d4)',
              }}>
                {selected.content}
              </pre>
            </div>
          ) : selected.id === 'erd-pdf' ? (
            // PDF info panel
            <div style={{ padding: 24 }}>
              <div style={{
                padding: '16px 20px', borderRadius: 8,
                background: 'rgba(99,102,241,0.08)',
                border: '1px solid rgba(99,102,241,0.2)',
                fontSize: 13, lineHeight: 1.8,
                color: 'var(--vscode-editor-foreground, #d4d4d4)',
              }}>
                {selected.content}
              </div>
            </div>
          ) : (
            // Code preview with line numbers
            selected.content.split('\n').map((line, i) => (
              <div key={i} style={{ display: 'flex', minHeight: 20 }}>
                <span style={{
                  minWidth: 44, paddingRight: 10, textAlign: 'right', flexShrink: 0,
                  fontSize: 11, lineHeight: '20px', userSelect: 'none',
                  color: 'var(--vscode-editorLineNumber-foreground, rgba(255,255,255,0.2))',
                }}>{i + 1}</span>
                <span style={{
                  flex: 1, paddingRight: 16, lineHeight: '20px', whiteSpace: 'pre',
                  color: 'var(--vscode-editor-foreground, #d4d4d4)',
                }}>{line}</span>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

// ── FileRow sub-component ─────────────────────────────────────────────────────

function FileRow({
  f, selected, onClick,
}: {
  f: ExportFile;
  selected: ExportFile;
  onClick: () => void;
}) {
  const isSel = selected.id === f.id;
  return (
    <div
      onClick={onClick}
      style={{
        display: 'flex', alignItems: 'center', gap: 6,
        padding: '5px 10px', cursor: 'pointer',
        background: isSel ? 'rgba(99,102,241,0.15)' : 'transparent',
        borderLeft: isSel
          ? '2px solid var(--vscode-focusBorder, #007acc)'
          : '2px solid transparent',
      }}
    >
      <span style={{ fontSize: 10, opacity: 0.3, flexShrink: 0 }}>
        {f.ext === '.sql' ? '≡' : f.ext === '.svg' || f.ext === '.pdf' ? '◫' : '{ }'}
      </span>
      <span style={{ flex: 1, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {f.label}<span style={{ opacity: 0.35 }}>{f.ext}</span>
      </span>
      {f.isDummy && (
        <span style={{ fontSize: 9, opacity: 0.4 }}>~</span>
      )}
    </div>
  );
}
