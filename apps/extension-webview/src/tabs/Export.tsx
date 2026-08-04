import React, { useState } from 'react';
import { Box, Flex, Text, VStack } from '@devup-ui/react';
import { postMessage } from '../vscode';
import type { AppState } from '../App';
import { DEFAULT_SCHEMAS } from '../App';
import type { OrmType, WebviewMessage } from '../vscode';

// ── Types ─────────────────────────────────────────────────────────────────────

interface ExportFile {
  id: string;
  label: string;
  ext: string;
  lang: string;
  content: string;
  isDummy: boolean;
}

const DUMMY_SQL_PG = `-- PostgreSQL migration (preview)\n\nCREATE TABLE "users" (\n  "id" SERIAL NOT NULL,\n  "email" TEXT NOT NULL,\n  "name" TEXT,\n  "created_at" TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW(),\n  CONSTRAINT "pk_users" PRIMARY KEY ("id"),\n  CONSTRAINT "uq_users__email" UNIQUE ("email")\n);\n\nCREATE TABLE "posts" (\n  "id" SERIAL NOT NULL,\n  "title" TEXT NOT NULL,\n  "content" TEXT,\n  "published" BOOLEAN NOT NULL DEFAULT false,\n  "author_id" INTEGER NOT NULL,\n  CONSTRAINT "pk_posts" PRIMARY KEY ("id"),\n  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")\n);\n\nCREATE INDEX "ix_posts__author_id" ON "posts" ("author_id");`;

const DUMMY_SQL_MY = `-- MySQL migration (preview)\n\nCREATE TABLE \`users\` (\n  \`id\` INT NOT NULL AUTO_INCREMENT,\n  \`email\` VARCHAR(191) NOT NULL,\n  \`name\` VARCHAR(191),\n  PRIMARY KEY (\`id\`),\n  CONSTRAINT \`uq_users__email\` UNIQUE (\`email\`)\n) ENGINE=InnoDB;\n\nCREATE TABLE \`posts\` (\n  \`id\` INT NOT NULL AUTO_INCREMENT,\n  \`title\` VARCHAR(191) NOT NULL,\n  \`author_id\` INT NOT NULL,\n  PRIMARY KEY (\`id\`),\n  CONSTRAINT \`fk_posts__author_id\` FOREIGN KEY (\`author_id\`) REFERENCES \`users\` (\`id\`)\n) ENGINE=InnoDB;`;

const DUMMY_SQL_SQ = `-- SQLite migration (preview)\n\nCREATE TABLE "users" (\n  "id" INTEGER NOT NULL,\n  "email" TEXT NOT NULL,\n  "name" TEXT,\n  CONSTRAINT "pk_users" PRIMARY KEY ("id" AUTOINCREMENT),\n  CONSTRAINT "uq_users__email" UNIQUE ("email")\n);\n\nCREATE TABLE "posts" (\n  "id" INTEGER NOT NULL,\n  "title" TEXT NOT NULL,\n  "author_id" INTEGER NOT NULL,\n  CONSTRAINT "pk_posts" PRIMARY KEY ("id" AUTOINCREMENT),\n  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")\n);`;

const DUMMY_SVG = `<!-- ERD Diagram (preview) -->\n<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200" viewBox="0 0 400 200">\n  <rect x="20" y="20" width="160" height="80" rx="8"\n    fill="#1e1e2e" stroke="#6366f1" stroke-width="1.5"/>\n  <text x="100" y="65" font-family="sans-serif" font-size="13"\n    fill="#a5b4fc" text-anchor="middle">users</text>\n  <rect x="220" y="20" width="160" height="80" rx="8"\n    fill="#1e1e2e" stroke="#8b5cf6" stroke-width="1.5"/>\n  <text x="300" y="65" font-family="sans-serif" font-size="13"\n    fill="#c4b5fd" text-anchor="middle">posts</text>\n  <path d="M180 60 C200 60, 200 60, 220 60"\n    fill="none" stroke="rgba(99,102,241,0.6)" stroke-width="1.5"\n    stroke-dasharray="5 3"/>\n</svg>`;

// ── Export actions ────────────────────────────────────────────────────────────

const EXPORT_ACTIONS: Record<string, (file: ExportFile, ormType: OrmType) => WebviewMessage> = {
  'sql-pg':      (file) => ({ type: 'export_sql', content: file.content, dialect: 'postgres' }),
  'sql-my':      (file) => ({ type: 'export_sql', content: file.content, dialect: 'mysql' }),
  'sql-sq':      (file) => ({ type: 'export_sql', content: file.content, dialect: 'sqlite' }),
  'orm-src':     (file, ormType) => ({ type: 'export_schema', content: file.content, ormType }),
  'schema-json': (file) => ({ type: 'export_schema', content: file.content, ormType: 'prisma' }),
  'erd-svg':     () => ({ type: 'export_svg' }),
  'erd-pdf':     () => ({ type: 'export_pdf' }),
};

function saveFile(file: ExportFile | undefined, ormType: OrmType) {
  if (!file) return;
  const buildMessage = EXPORT_ACTIONS[file.id];
  if (buildMessage) postMessage(buildMessage(file, ormType));
}

// ── Component ─────────────────────────────────────────────────────────────────

interface Props { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> }

export default function Export({ state }: Props) {
  const [panelId, setPanelId] = useState('sql-pg');
  const [copied, setCopied]   = useState<string | null>(null);

  const hasSql    = !!(state.postgres || state.mysql || state.sqlite);
  const hasSvg    = !!state.svg;
  const hasSchema = Object.keys(state.schema ?? {}).length > 0;
  const ormLabel  = state.ormType.charAt(0).toUpperCase() + state.ormType.slice(1);

  const files: ExportFile[] = [
    { id: 'sql-pg',    label: 'migration.postgres', ext: '.sql',    lang: 'SQL',     content: state.postgres || DUMMY_SQL_PG, isDummy: !hasSql },
    { id: 'sql-my',    label: 'migration.mysql',    ext: '.sql',    lang: 'SQL',     content: state.mysql    || DUMMY_SQL_MY, isDummy: !hasSql },
    { id: 'sql-sq',    label: 'migration.sqlite',   ext: '.sql',    lang: 'SQL',     content: state.sqlite   || DUMMY_SQL_SQ, isDummy: !hasSql },
    { id: 'orm-src',   label: `schema.${state.ormType}`, ext: state.ormType === 'prisma' ? '.prisma' : state.ormType === 'drizzle' || state.ormType === 'typeorm' ? '.ts' : state.ormType === 'gorm' ? '.go' : '.java', lang: ormLabel, content: state.ormSource || DEFAULT_SCHEMAS[state.ormType], isDummy: !state.ormSource },
    { id: 'schema-json', label: 'schema', ext: '.json', lang: 'JSON', content: hasSchema ? JSON.stringify(state.schema, null, 2) : '{}', isDummy: !hasSchema },
    { id: 'erd-svg',   label: 'erd-diagram', ext: '.svg', lang: 'SVG', content: state.svg || DUMMY_SVG, isDummy: !hasSvg },
    { id: 'erd-pdf',   label: 'erd-diagram', ext: '.pdf', lang: 'PDF', content: '', isDummy: !hasSvg },
  ];

  const selectedFile = files.find((f) => f.id === panelId);

  function copyContent(content: string, id: string) {
    navigator.clipboard.writeText(content).then(() => {
      setCopied(id);
      setTimeout(() => setCopied(null), 1500);
    });
  }

  return (
    <Flex h="100%" overflow="hidden">

      {/* ── Left sidebar ── */}
      <VStack
        minW="220px"
        flexShrink={0}
        borderRight="1px solid $border"
        bg="$sidebarBg"
        overflow="hidden"
      >
        <Box flex={1} overflow="auto" minH={0}>
          <SectionHeader label="EXPORT FILES" />

          <GroupLabel label="MIGRATION SQL" />
          {files.filter((f) => f.id.startsWith('sql-')).map((f) => (
            <FileRow key={f.id} f={f} active={panelId === f.id}
              onClick={() => setPanelId(f.id)} />
          ))}

          <GroupLabel label="SCHEMA" />
          {files.filter((f) => f.id === 'orm-src' || f.id === 'schema-json').map((f) => (
            <FileRow key={f.id} f={f} active={panelId === f.id}
              onClick={() => setPanelId(f.id)} />
          ))}

          <GroupLabel label="DIAGRAM" />
          {files.filter((f) => f.id.startsWith('erd-')).map((f) => (
            <FileRow key={f.id} f={f} active={panelId === f.id}
              onClick={() => setPanelId(f.id)} />
          ))}
        </Box>
      </VStack>

      {/* ── Right panel ── */}
      <VStack flex={1} overflow="hidden">
        {selectedFile && (
          <>
            <FileHeader
              file={selectedFile}
              copied={copied === selectedFile.id}
              onCopy={() => copyContent(selectedFile.content, selectedFile.id)}
              onSave={() => saveFile(selectedFile, state.ormType)}
            />
            <FilePreviewBody file={selectedFile} />
          </>
        )}
      </VStack>
    </Flex>
  );
}

// ── Sub-components ─────────────────────────────────────────────────────────────

function SectionHeader({ label }: { label: string }) {
  return (
    <Text as="div" py="8px" px="10px" pb="4px" fontSize="10px" fontWeight={700} letterSpacing="0.08em" color="var(--node-text-dim)" flexShrink={0}>
      {label}
    </Text>
  );
}

function GroupLabel({ label }: { label: string }) {
  return (
    <Text as="div" py="6px" px="10px" pb="2px" fontSize="9px" fontWeight={600} color="var(--node-text-dim)" letterSpacing="0.06em">
      {label}
    </Text>
  );
}

function FileRow({ f, active, onClick }: { f: ExportFile; active: boolean; onClick: () => void }) {
  return (
    <Flex
      onClick={onClick}
      alignItems="center"
      gap="6px"
      py="4px"
      px="10px"
      cursor="pointer"
      bg={active ? 'rgba(99,102,241,0.15)' : 'transparent'}
      borderLeft={active ? '2px solid $focusBorder' : '2px solid transparent'}
    >
      <Box as="span" fontSize="10px" color="var(--node-text-dim)" flexShrink={0}>
        {f.ext === '.sql' ? '≡' : f.ext === '.svg' || f.ext === '.pdf' ? '◫' : '{ }'}
      </Box>
      <Box as="span" flex={1} fontSize="12px" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap" color="var(--node-text)">
        {f.label}<Box as="span" color="var(--node-text-dim)">{f.ext}</Box>
      </Box>
      {f.isDummy && <Box as="span" fontSize="9px" color="var(--node-text-dim)">~</Box>}
    </Flex>
  );
}

function FileHeader({ file, copied, onCopy, onSave }: {
  file: ExportFile; copied: boolean; onCopy: () => void; onSave: () => void;
}) {
  return (
    <Flex
      alignItems="center"
      gap="8px"
      py="5px"
      px="12px"
      flexShrink={0}
      borderBottom="1px solid $border"
      bg="$tabsBg"
      fontSize="12px"
    >
      <Box as="span" fontWeight={600}>{file.label}</Box>
      <Box as="span" color="var(--node-text-dim)">{file.ext}</Box>
      <Box
        as="span"
        fontSize="9px"
        py="1px"
        px="6px"
        borderRadius="3px"
        bg="rgba(99,102,241,0.15)"
        color="#a5b4fc"
        border="1px solid rgba(99,102,241,0.25)"
        fontWeight={700}
      >{file.lang}</Box>
      {file.isDummy && (
        <Box
          as="span"
          fontSize="9px"
          py="1px"
          px="6px"
          borderRadius="3px"
          bg="rgba(251,191,36,0.12)"
          color="#fbbf24"
          border="1px solid rgba(251,191,36,0.25)"
        >PREVIEW</Box>
      )}
      <Box flex={1} />
      <Box as="button" onClick={onCopy} {...btnStyle(copied ? 'green' : 'default')}>
        {copied ? '✓ 복사됨' : '복사'}
      </Box>
      <Box as="button" onClick={onSave} {...btnStyle('primary')}>저장</Box>
    </Flex>
  );
}

function FilePreviewBody({ file }: { file: ExportFile }) {
  if (file.id === 'erd-pdf') {
    return (
      <Box flex={1} overflow="auto" p="24px" bg="$editorBg">
        <Box
          py="16px"
          px="20px"
          borderRadius="8px"
          bg="rgba(99,102,241,0.08)"
          border="1px solid rgba(99,102,241,0.2)"
          fontSize="13px"
          lineHeight={1.8}
          color="$editorFg"
        >
          PDF export converts the ERD diagram SVG to a portable document.{'\n\n'}
          Click "저장" to generate the PDF file.{'\n'}
          {file.isDummy ? '⚠ ORM Editor에서 스키마를 먼저 입력하세요.' : '✓ ERD 준비 완료.'}
        </Box>
      </Box>
    );
  }

  if (file.id === 'erd-svg' && file.content.startsWith('<svg')) {
    return (
      <Box flex={1} overflow="auto" p="24px" bg="$editorBg">
        <Box dangerouslySetInnerHTML={{ __html: file.content }} maxWidth="100%" />
        <Box mt="16px" fontSize="11px" opacity={0.4}>SVG 소스:</Box>
        <Box
          as="pre"
          mt="8px"
          py="10px"
          px="14px"
          bg="rgba(0,0,0,0.2)"
          borderRadius="6px"
          fontSize="11px"
          lineHeight={1.6}
          whiteSpace="pre"
          overflowX="auto"
          color="$editorFg"
        >{file.content}</Box>
      </Box>
    );
  }

  return (
    <Box
      flex={1}
      overflow="auto"
      bg="$editorBg"
      fontFamily="$editorFont"
      fontSize="12px"
      lineHeight="20px"
    >
      {file.content.split('\n').map((line, i) => (
        <Flex key={i} minH="20px">
          <Box
            as="span"
            minW="44px"
            pr="10px"
            textAlign="right"
            flexShrink={0}
            fontSize="11px"
            lineHeight="20px"
            userSelect="none"
            color="var(--diff-linenum)"
          >{i + 1}</Box>
          <Box as="span" flex={1} pr="16px" lineHeight="20px" whiteSpace="pre" color="$editorFg">{line}</Box>
        </Flex>
      ))}
    </Box>
  );
}

// ── Style helpers ─────────────────────────────────────────────────────────────

function btnStyle(variant: 'primary' | 'default' | 'green') {
  const base = {
    border: 'none' as const,
    borderRadius: '3px',
    py: '3px',
    px: '12px',
    fontSize: '11px',
    cursor: 'pointer' as const,
    fontFamily: 'inherit',
    flexShrink: 0,
  };
  if (variant === 'primary') return { ...base, bg: '$btnBg', color: '$btnFg' };
  if (variant === 'green')   return { ...base, bg: 'rgba(74,222,128,0.12)', color: '#4ade80', border: '1px solid rgba(74,222,128,0.25)' };
  return { ...base, bg: 'transparent', color: 'var(--node-text)', border: '1px solid var(--node-border)' };
}
