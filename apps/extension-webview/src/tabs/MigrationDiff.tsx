import React, { useEffect, useRef, useState } from 'react';
import { Box, Flex } from '@devup-ui/react';
import { postMessage } from '../vscode';
import type { AppState } from '../App';

// ── Types ─────────────────────────────────────────────────────────────────────

type Dialect  = 'postgres' | 'mysql' | 'sqlite';
type FileKind = 'create' | 'alter' | 'drop' | 'index';

interface SqlFile {
  id:        string;
  name:      string;
  kind:      FileKind;
  sql:       string;
  adds:      number;
  removes:   number;
  startLine: number;
}

interface DiffLine {
  type:   'add' | 'remove' | 'ctx';
  oldNum: number | null;
  newNum: number | null;
  text:   string;
}

// ── Pre-built dummy files ──────────────────────────────────────────────────────

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

const PG_ALTER_USERS = `ALTER TABLE "users"
  ADD COLUMN "phone" TEXT,
  ADD COLUMN "avatar_url" TEXT,
  DROP COLUMN "name",
  DROP COLUMN "created_at";`;

const MY_ALTER_USERS = `ALTER TABLE \`users\`
  ADD COLUMN \`phone\` VARCHAR(191),
  ADD COLUMN \`avatar_url\` VARCHAR(512),
  DROP COLUMN \`name\`,
  DROP COLUMN \`created_at\`;`;

const SQ_ALTER_USERS = `ALTER TABLE "users"
  ADD COLUMN "phone" TEXT,
  ADD COLUMN "avatar_url" TEXT,
  DROP COLUMN "name",
  DROP COLUMN "created_at";`;

const PG_DROP_SESSIONS = `DROP TABLE IF EXISTS "sessions";`;
const MY_DROP_SESSIONS = `DROP TABLE IF EXISTS \`sessions\`;`;
const SQ_DROP_SESSIONS = `DROP TABLE IF EXISTS "sessions";`;

function makeFile(id: string, name: string, kind: FileKind, sql: string, startLine: number): SqlFile {
  const allLines = sql.split('\n');
  const adds    = kind === 'create' || kind === 'index' ? allLines.filter(l => l.trim()).length : allLines.filter(l => /^\s+ADD/i.test(l)).length;
  const removes = kind === 'drop' ? allLines.filter(l => l.trim()).length : allLines.filter(l => /^\s+DROP/i.test(l)).length;
  return { id, name, kind, sql, adds, removes, startLine };
}

function buildDummyFiles(items: Array<[string, string, FileKind, string]>): SqlFile[] {
  let cursor = 1;
  return items.map(([id, name, kind, sql]) => {
    const file = makeFile(id, name, kind, sql, cursor);
    cursor += sql.split('\n').length + 1;
    return file;
  });
}

const DUMMY_FILES: Record<Dialect, SqlFile[]> = {
  postgres: buildDummyFiles([
    ['0', 'users',        'create', PG_USERS],
    ['1', 'profiles',     'create', PG_PROFILES],
    ['2', 'posts',        'create', PG_POSTS],
    ['3', 'tags',         'create', PG_TAGS],
    ['4', 'tag_on_posts', 'create', PG_TAG_ON_POSTS],
    ['5', 'indexes',      'index',  PG_INDEXES],
    ['6', 'users',        'alter',  PG_ALTER_USERS],
    ['7', 'sessions',     'drop',   PG_DROP_SESSIONS],
  ]),
  mysql: buildDummyFiles([
    ['0', 'users',        'create', MY_USERS],
    ['1', 'profiles',     'create', MY_PROFILES],
    ['2', 'posts',        'create', MY_POSTS],
    ['3', 'tags',         'create', MY_TAGS],
    ['4', 'tag_on_posts', 'create', MY_TAG_ON_POSTS],
    ['5', 'indexes',      'index',  MY_INDEXES],
    ['6', 'users',        'alter',  MY_ALTER_USERS],
    ['7', 'sessions',     'drop',   MY_DROP_SESSIONS],
  ]),
  sqlite: buildDummyFiles([
    ['0', 'users',        'create', SQ_USERS],
    ['1', 'profiles',     'create', SQ_PROFILES],
    ['2', 'posts',        'create', SQ_POSTS],
    ['3', 'tags',         'create', SQ_TAGS],
    ['4', 'tag_on_posts', 'create', SQ_TAG_ON_POSTS],
    ['5', 'indexes',      'index',  SQ_INDEXES],
    ['6', 'users',        'alter',  SQ_ALTER_USERS],
    ['7', 'sessions',     'drop',   SQ_DROP_SESSIONS],
  ]),
};

// ── SQL parser ────────────────────────────────────────────────────────────────

function stripQuotes(s: string) {
  return s.replace(/^["`[`]/, '').replace(/["`\]]$/, '');
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
  const rawParts = sql.split(/;[ \t]*\n/);
  let lineOffset = 1;
  const stmtInfos: Array<{ stmt: string; startLine: number }> = [];
  for (const part of rawParts) {
    const trimmed = part.trim();
    if (trimmed) {
      stmtInfos.push({ stmt: trimmed.endsWith(';') ? trimmed : trimmed + ';', startLine: lineOffset });
    }
    lineOffset += part.split('\n').length;
  }

  const byKey = new Map<string, SqlFile>();
  let idx = 0;

  for (const { stmt, startLine } of stmtInfos) {
    const up = stmt.trimStart().toUpperCase();
    let kind: FileKind;
    let name: string;

    if (up.startsWith('CREATE TABLE'))    { kind = 'create'; name = extractName(stmt, 'CREATE TABLE'); }
    else if (up.startsWith('ALTER TABLE')) { kind = 'alter';  name = extractName(stmt, 'ALTER TABLE'); }
    else if (up.startsWith('DROP TABLE'))  { kind = 'drop';   name = extractName(stmt, 'DROP TABLE'); }
    else if (up.includes('INDEX'))         { kind = 'index';  name = 'indexes'; }
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
      byKey.set(key, { id: String(idx++), name, kind, sql: stmt, adds, removes, startLine });
    }
  }
  return Array.from(byKey.values());
}

// ── Diff line generator ───────────────────────────────────────────────────────

function toDiffLines(file: SqlFile): DiffLine[] {
  let oldN = file.startLine, newN = file.startLine;
  return file.sql.split('\n').map((text) => {
    let type: DiffLine['type'] = 'ctx';
    if (file.kind === 'create' || file.kind === 'index') {
      type = 'add';
    } else if (file.kind === 'drop') {
      type = 'remove';
    } else {
      const t = text.trim().toUpperCase();
      if (t.startsWith('ADD'))       type = 'add';
      else if (t.startsWith('DROP')) type = 'remove';
    }
    if (type === 'add')    return { type, oldNum: null,   newNum: newN++, text };
    if (type === 'remove') return { type, oldNum: oldN++, newNum: null,   text };
    return                        { type, oldNum: oldN++, newNum: newN++, text };
  });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const DIALECT_LABELS: Record<Dialect, string> = { postgres: 'PG', mysql: 'MY', sqlite: 'SQ' };

function kindBadge(kind: FileKind) {
  if (kind === 'create') return { label: 'A', color: 'var(--diff-add-sign)' };
  if (kind === 'drop')   return { label: 'D', color: 'var(--diff-rm-sign)' };
  if (kind === 'index')  return { label: 'I', color: '#3b82f6' };
  return { label: 'M', color: '#d97706' };
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

  const realSql =
    dialect === 'postgres' ? state.postgres :
    dialect === 'mysql'    ? state.mysql    : state.sqlite;

  const hasReal = !!realSql;
  const files   = hasReal ? parseSql(realSql) : DUMMY_FILES[dialect];

  useEffect(() => { setSelectedId('0'); }, [dialect]);

  const selected  = files.find((f) => f.id === selectedId) ?? files[0];
  const diffLines = selected ? toDiffLines(selected) : [];

  const totalAdds    = files.reduce((s, f) => s + f.adds,    0);
  const totalRemoves = files.reduce((s, f) => s + f.removes, 0);

  return (
    <Flex h="100%" overflow="hidden">

      {/* ── Left: file list ── */}
      <Flex
        w="224px"
        flexShrink={0}
        flexDir="column"
        borderRight="1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))"
        bg="var(--diff-sidebar-bg, #252526)"
        color="var(--diff-sidebar-text, #cccccc)"
      >
        {/* Header */}
        <Flex
          py="8px"
          px="10px"
          pb="6px"
          flexShrink={0}
          alignItems="center"
          justifyContent="space-between"
          borderBottom="1px solid var(--vscode-panel-border, rgba(255,255,255,0.08))"
        >
          <Box as="span" fontSize="10px" fontWeight={700} letterSpacing="0.08em" color="var(--node-text-dim)">
            CHANGES
          </Box>
          {/* Dialect toggle */}
          <Flex
            gap="1px"
            p="2px"
            bg="var(--vscode-editorWidget-background, rgba(0,0,0,0.25))"
            borderRadius="5px"
            border="1px solid var(--node-border)"
          >
            {(['postgres', 'mysql', 'sqlite'] as Dialect[]).map((d) => (
              <Box
                key={d}
                as="button"
                onClick={() => setDialect(d)}
                py="2px"
                px="7px"
                border="none"
                borderRadius="4px"
                cursor="pointer"
                fontSize="10px"
                fontWeight={600}
                bg={dialect === d ? 'var(--vscode-button-background, #0e639c)' : 'transparent'}
                color={dialect === d ? '#fff' : 'var(--node-text-dim)'}
                transition="background 0.1s"
              >{DIALECT_LABELS[d]}</Box>
            ))}
          </Flex>
        </Flex>

        {/* Summary */}
        <Flex py="4px" px="10px" fontSize="10px" gap="6px" alignItems="center" color="var(--node-text-dim)" flexShrink={0}>
          <span>{files.length} files</span>
          {totalAdds    > 0 && <Box as="span" color="var(--diff-add-sign)">+{totalAdds}</Box>}
          {totalRemoves > 0 && <Box as="span" color="var(--diff-rm-sign)">−{totalRemoves}</Box>}
          {!hasReal && (
            <Box
              as="span"
              ml="auto"
              fontSize="9px"
              py="1px"
              px="5px"
              borderRadius="3px"
              bg="rgba(251,191,36,0.15)"
              color="#fbbf24"
              border="1px solid rgba(251,191,36,0.28)"
            >PREVIEW</Box>
          )}
        </Flex>

        {/* File list */}
        <Box flex={1} overflowY="auto">
          {files.map((f) => {
            const isSel = f.id === selectedId;
            const badge = kindBadge(f.kind);
            return (
              <Flex
                key={f.id}
                onClick={() => setSelectedId(f.id)}
                alignItems="center"
                gap="6px"
                py="5px"
                px="10px"
                cursor="pointer"
                bg={isSel ? 'rgba(99,102,241,0.15)' : 'transparent'}
                borderLeft={isSel ? '2px solid var(--vscode-focusBorder, #007acc)' : '2px solid transparent'}
              >
                <Box as="span" fontSize="11px" color="var(--node-text-dim)" flexShrink={0}>
                  {f.kind === 'index' ? '⊞' : '≡'}
                </Box>
                <Box as="span" flex={1} fontSize="12px" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap" color="var(--diff-sidebar-text)">
                  {f.name}<Box as="span" color="var(--node-text-dim)">.sql</Box>
                </Box>
                {f.adds    > 0 && <Box as="span" fontSize="10px" color="var(--diff-add-sign)" flexShrink={0} fontWeight={600}>+{f.adds}</Box>}
                {f.removes > 0 && <Box as="span" fontSize="10px" color="var(--diff-rm-sign)"  flexShrink={0} fontWeight={600}>−{f.removes}</Box>}
                <Box as="span" fontSize="10px" fontWeight={700} color={badge.color} w="12px" textAlign="right" flexShrink={0}>
                  {badge.label}
                </Box>
              </Flex>
            );
          })}
        </Box>
      </Flex>

      {/* ── Right: diff viewer ── */}
      <Flex flex={1} flexDir="column" overflow="hidden">

        {/* File header */}
        {selected && (
          <Flex
            alignItems="center"
            gap="8px"
            py="5px"
            px="12px"
            flexShrink={0}
            borderBottom="1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))"
            bg="var(--diff-header-bg, #2d2d2d)"
            color="var(--diff-header-text, #cccccc)"
            fontSize="12px"
          >
            <Box as="span" color="var(--node-text-dim)">{selected.kind === 'index' ? '⊞' : '≡'}</Box>
            <Box as="span" fontWeight={600}>{selected.name}</Box>
            <Box as="span" color="var(--node-text-dim)">.sql</Box>
            <Box flex={1} />
            {selected.adds    > 0 && <Box as="span" fontSize="11px" color="var(--diff-add-sign)" fontWeight={600}>+{selected.adds}</Box>}
            {selected.removes > 0 && <Box as="span" fontSize="11px" color="var(--diff-rm-sign)"  fontWeight={600}>−{selected.removes}</Box>}
            <Box
              as="button"
              onClick={() => navigator.clipboard.writeText(selected.sql).catch(console.error)}
              bg="transparent"
              cursor="pointer"
              border="1px solid var(--node-border)"
              borderRadius="3px"
              color="var(--diff-header-text)"
              py="1px"
              px="8px"
              fontSize="10px"
            >복사</Box>
          </Flex>
        )}

        {/* Diff lines */}
        <Box
          flex={1}
          overflow="auto"
          bg="var(--diff-bg, #1e1e1e)"
          fontFamily="var(--vscode-editor-font-family, Consolas, 'Courier New', monospace)"
          fontSize="12px"
          lineHeight="20px"
        >
          {diffLines.map((line, i) => (
            <Flex
              key={i}
              minH="20px"
              bg={
                line.type === 'add'    ? 'var(--diff-add-bg)'  :
                line.type === 'remove' ? 'var(--diff-rm-bg)'   : 'transparent'
              }
              borderLeft={
                line.type === 'add'    ? '3px solid var(--diff-add-border)'  :
                line.type === 'remove' ? '3px solid var(--diff-rm-border)'   : '3px solid transparent'
              }
            >
              {/* Old line number */}
              <Box
                as="span"
                minW="40px"
                pr="6px"
                textAlign="right"
                flexShrink={0}
                fontSize="11px"
                lineHeight="20px"
                userSelect="none"
                color="var(--diff-linenum)"
              >{line.oldNum ?? ''}</Box>
              {/* New line number */}
              <Box
                as="span"
                minW="40px"
                pr="8px"
                textAlign="right"
                flexShrink={0}
                fontSize="11px"
                lineHeight="20px"
                userSelect="none"
                color="var(--diff-linenum)"
                borderRight="1px solid var(--vscode-panel-border, rgba(128,128,128,0.2))"
                mr="4px"
              >{line.newNum ?? ''}</Box>
              {/* +/- sign */}
              <Box
                as="span"
                w="18px"
                flexShrink={0}
                textAlign="center"
                lineHeight="20px"
                fontSize="12px"
                fontWeight={700}
                userSelect="none"
                color={
                  line.type === 'add'    ? 'var(--diff-add-sign)' :
                  line.type === 'remove' ? 'var(--diff-rm-sign)'  : 'transparent'
                }
              >
                {line.type === 'add' ? '+' : line.type === 'remove' ? '−' : ' '}
              </Box>
              {/* Code text */}
              <Box
                as="span"
                flex={1}
                pr="16px"
                lineHeight="20px"
                whiteSpace="pre"
                color={
                  line.type === 'add'    ? 'var(--diff-add-text)' :
                  line.type === 'remove' ? 'var(--diff-rm-text)'  :
                  'var(--diff-text)'
                }
              >{line.text}</Box>
            </Flex>
          ))}
        </Box>
      </Flex>
    </Flex>
  );
}
