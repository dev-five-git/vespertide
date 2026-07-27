import React, { useState, useEffect, useRef } from 'react';
import { Box, Flex, VStack } from '@devup-ui/react';
import { postMessage, onMessage } from '../vscode';
import type { AppState } from '../App';
import { DEFAULT_SCHEMAS } from '../App';
import type { ConnectorService, ConnectorStatus, ChatMessage } from '../vscode';

// ── Types ─────────────────────────────────────────────────────────────────────

type RightPanel =
  | { kind: 'file'; id: string }
  | { kind: 'connector'; service: ConnectorService }
  | { kind: 'chat' };

type ExportFile = {
  id: string;
  label: string;
  ext: string;
  lang: string;
  content: string;
  isDummy: boolean;
};

type ConnectorMeta = {
  service: ConnectorService;
  label: string;
  icon: string;
  subtitle?: string;
  keyLabel: string;
  keyPlaceholder: string;
  keyHelp?: string;
  isAI: boolean;
};

const CONNECTORS: ConnectorMeta[] = [
  {
    service: 'claude', label: 'Claude', icon: '🤖', subtitle: 'Anthropic',
    keyLabel: 'API Key', keyPlaceholder: 'sk-ant-api03-...', isAI: true,
    keyHelp: 'console.anthropic.com에서 발급',
  },
  {
    service: 'openai', label: 'OpenAI / GPT', icon: '🧠', subtitle: 'OpenAI',
    keyLabel: 'API Key', keyPlaceholder: 'sk-proj-...', isAI: true,
    keyHelp: 'platform.openai.com에서 발급',
  },
  {
    service: 'gemini', label: 'Gemini', icon: '✦', subtitle: 'Google',
    keyLabel: 'API Key', keyPlaceholder: 'AIzaSy...', isAI: true,
    keyHelp: 'aistudio.google.com에서 발급',
  },
  {
    service: 'slack', label: 'Slack', icon: '💬', subtitle: 'Workspace',
    keyLabel: 'Webhook URL', keyPlaceholder: 'https://hooks.slack.com/services/...', isAI: false,
    keyHelp: 'Slack 앱 설정 → Incoming Webhooks',
  },
  {
    service: 'notion', label: 'Notion', icon: '📝', subtitle: 'Workspace',
    keyLabel: 'Integration Token', keyPlaceholder: 'secret_...', isAI: false,
    keyHelp: 'notion.so/my-integrations에서 발급',
  },
  {
    service: 'jira', label: 'Jira', icon: '🎯', subtitle: 'Atlassian',
    keyLabel: 'Email:API Token', keyPlaceholder: 'user@example.com:token...', isAI: false,
    keyHelp: 'id.atlassian.com/manage-profile/security/api-tokens',
  },
];

const DUMMY_SQL_PG = `-- PostgreSQL migration (preview)\n\nCREATE TABLE "users" (\n  "id" SERIAL NOT NULL,\n  "email" TEXT NOT NULL,\n  "name" TEXT,\n  "created_at" TIMESTAMP WITHOUT TIME ZONE DEFAULT NOW(),\n  CONSTRAINT "pk_users" PRIMARY KEY ("id"),\n  CONSTRAINT "uq_users__email" UNIQUE ("email")\n);\n\nCREATE TABLE "posts" (\n  "id" SERIAL NOT NULL,\n  "title" TEXT NOT NULL,\n  "content" TEXT,\n  "published" BOOLEAN NOT NULL DEFAULT false,\n  "author_id" INTEGER NOT NULL,\n  CONSTRAINT "pk_posts" PRIMARY KEY ("id"),\n  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")\n);\n\nCREATE INDEX "ix_posts__author_id" ON "posts" ("author_id");`;

const DUMMY_SQL_MY = `-- MySQL migration (preview)\n\nCREATE TABLE \`users\` (\n  \`id\` INT NOT NULL AUTO_INCREMENT,\n  \`email\` VARCHAR(191) NOT NULL,\n  \`name\` VARCHAR(191),\n  PRIMARY KEY (\`id\`),\n  CONSTRAINT \`uq_users__email\` UNIQUE (\`email\`)\n) ENGINE=InnoDB;\n\nCREATE TABLE \`posts\` (\n  \`id\` INT NOT NULL AUTO_INCREMENT,\n  \`title\` VARCHAR(191) NOT NULL,\n  \`author_id\` INT NOT NULL,\n  PRIMARY KEY (\`id\`),\n  CONSTRAINT \`fk_posts__author_id\` FOREIGN KEY (\`author_id\`) REFERENCES \`users\` (\`id\`)\n) ENGINE=InnoDB;`;

const DUMMY_SQL_SQ = `-- SQLite migration (preview)\n\nCREATE TABLE "users" (\n  "id" INTEGER NOT NULL,\n  "email" TEXT NOT NULL,\n  "name" TEXT,\n  CONSTRAINT "pk_users" PRIMARY KEY ("id" AUTOINCREMENT),\n  CONSTRAINT "uq_users__email" UNIQUE ("email")\n);\n\nCREATE TABLE "posts" (\n  "id" INTEGER NOT NULL,\n  "title" TEXT NOT NULL,\n  "author_id" INTEGER NOT NULL,\n  CONSTRAINT "pk_posts" PRIMARY KEY ("id" AUTOINCREMENT),\n  CONSTRAINT "fk_posts__author_id" FOREIGN KEY ("author_id") REFERENCES "users" ("id")\n);`;

const DUMMY_SVG = `<!-- ERD Diagram (preview) -->\n<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200" viewBox="0 0 400 200">\n  <rect x="20" y="20" width="160" height="80" rx="8"\n    fill="#1e1e2e" stroke="#6366f1" stroke-width="1.5"/>\n  <text x="100" y="65" font-family="sans-serif" font-size="13"\n    fill="#a5b4fc" text-anchor="middle">users</text>\n  <rect x="220" y="20" width="160" height="80" rx="8"\n    fill="#1e1e2e" stroke="#8b5cf6" stroke-width="1.5"/>\n  <text x="300" y="65" font-family="sans-serif" font-size="13"\n    fill="#c4b5fd" text-anchor="middle">posts</text>\n  <path d="M180 60 C200 60, 200 60, 220 60"\n    fill="none" stroke="rgba(99,102,241,0.6)" stroke-width="1.5"\n    stroke-dasharray="5 3"/>\n</svg>`;

// ── Component ─────────────────────────────────────────────────────────────────

type Props = { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> };

export default function Export({ state }: Props) {
  const [panel, setPanel]             = useState<RightPanel>({ kind: 'file', id: 'sql-pg' });
  const [connectors, setConnectors]   = useState<ConnectorStatus[]>([]);
  const [expandedConn, setExpandedConn] = useState<ConnectorService | null>(null);
  const [keyInputs, setKeyInputs]     = useState<Partial<Record<ConnectorService, string>>>({});
  const [saving, setSaving]           = useState<ConnectorService | null>(null);
  const [copied, setCopied]           = useState<string | null>(null);

  const [chatMessages, setChatMessages] = useState<ChatMessage[]>([]);
  const [chatInput, setChatInput]       = useState('');
  const [chatLoading, setChatLoading]   = useState(false);
  const [activeAI, setActiveAI]         = useState<ConnectorService>('claude');
  const chatEndRef = useRef<HTMLDivElement>(null) as React.RefObject<HTMLDivElement>;

  const hasSql    = !!(state.postgres || state.mysql || state.sqlite);
  const hasSvg    = !!state.svg;
  const hasSchema = Object.keys(state.schema ?? {}).length > 0;
  const ormLabel  = state.ormType.charAt(0).toUpperCase() + state.ormType.slice(1);

  useEffect(() => {
    postMessage({ type: 'connector_load' });
  }, []);

  useEffect(() => {
    const off = onMessage((msg) => {
      if (msg.type === 'connector_status') setConnectors(msg.connectors);
      if (msg.type === 'ai_response' && msg.done) {
        setChatMessages((prev) => [...prev, { role: 'assistant', content: msg.content }]);
        setChatLoading(false);
      }
      if (msg.type === 'error') {
        setChatMessages((prev) => [...prev, { role: 'assistant', content: `오류: ${msg.message}` }]);
        setChatLoading(false);
      }
    });
    return off;
  }, []);

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [chatMessages]);

  const files: ExportFile[] = [
    { id: 'sql-pg',    label: 'migration.postgres', ext: '.sql',    lang: 'SQL',     content: state.postgres || DUMMY_SQL_PG, isDummy: !hasSql },
    { id: 'sql-my',    label: 'migration.mysql',    ext: '.sql',    lang: 'SQL',     content: state.mysql    || DUMMY_SQL_MY, isDummy: !hasSql },
    { id: 'sql-sq',    label: 'migration.sqlite',   ext: '.sql',    lang: 'SQL',     content: state.sqlite   || DUMMY_SQL_SQ, isDummy: !hasSql },
    { id: 'orm-src',   label: `schema.${state.ormType}`, ext: state.ormType === 'prisma' ? '.prisma' : state.ormType === 'drizzle' || state.ormType === 'typeorm' ? '.ts' : state.ormType === 'gorm' ? '.go' : '.java', lang: ormLabel, content: state.ormSource || DEFAULT_SCHEMAS[state.ormType], isDummy: !state.ormSource },
    { id: 'schema-json', label: 'schema', ext: '.json', lang: 'JSON', content: hasSchema ? JSON.stringify(state.schema, null, 2) : '{}', isDummy: !hasSchema },
    { id: 'erd-svg',   label: 'erd-diagram', ext: '.svg', lang: 'SVG', content: state.svg || DUMMY_SVG, isDummy: !hasSvg },
    { id: 'erd-pdf',   label: 'erd-diagram', ext: '.pdf', lang: 'PDF', content: '', isDummy: !hasSvg },
  ];

  const selectedFile = files.find((f) => panel.kind === 'file' && f.id === panel.id);

  const connectedAIs = CONNECTORS.filter(
    (c) => c.isAI && connectors.find((s) => s.service === c.service)?.connected,
  );

  function saveFile() {
    if (!selectedFile) return;
    const id = selectedFile.id;
    if (id === 'sql-pg') postMessage({ type: 'export_sql', content: selectedFile.content, dialect: 'postgres' });
    else if (id === 'sql-my') postMessage({ type: 'export_sql', content: selectedFile.content, dialect: 'mysql' });
    else if (id === 'sql-sq') postMessage({ type: 'export_sql', content: selectedFile.content, dialect: 'sqlite' });
    else if (id === 'orm-src') postMessage({ type: 'export_schema', content: selectedFile.content, ormType: state.ormType });
    else if (id === 'erd-svg') postMessage({ type: 'export_svg' });
    else if (id === 'erd-pdf') postMessage({ type: 'export_pdf' });
    else if (id === 'schema-json') postMessage({ type: 'export_schema', content: selectedFile.content, ormType: 'prisma' });
  }

  function copyContent(content: string, id: string) {
    navigator.clipboard.writeText(content).then(() => {
      setCopied(id);
      setTimeout(() => setCopied(null), 1500);
    }).catch(console.error);
  }

  function toggleConnector(service: ConnectorService) {
    if (panel.kind === 'connector' && panel.service === service) {
      setPanel({ kind: 'file', id: 'sql-pg' });
    } else {
      setPanel({ kind: 'connector', service });
      setExpandedConn(service);
    }
  }

  function saveConnectorKey(service: ConnectorService) {
    const key = keyInputs[service]?.trim();
    if (!key) return;
    setSaving(service);
    postMessage({ type: 'connector_save', service, key });
    setKeyInputs((prev) => ({ ...prev, [service]: '' }));
    setTimeout(() => setSaving(null), 1500);
  }

  function disconnectConnector(service: ConnectorService) {
    postMessage({ type: 'connector_delete', service });
  }

  function sendChat() {
    const text = chatInput.trim();
    if (!text || chatLoading) return;
    setChatInput('');

    if (connectedAIs.length === 0) {
      setChatMessages((prev) => [
        ...prev,
        { role: 'user', content: text },
        { role: 'assistant', content: '왼쪽 CONNECTIONS에서 AI 서비스(Claude, OpenAI, Gemini)를 먼저 연결해주세요.' },
      ]);
      return;
    }

    const newMessages: ChatMessage[] = [...chatMessages, { role: 'user', content: text }];
    setChatMessages(newMessages);
    setChatLoading(true);
    const ctx = [
      state.ormSource ? `ORM Source:\n${state.ormSource}` : '',
      state.postgres  ? `PostgreSQL Migration:\n${state.postgres}` : '',
    ].filter(Boolean).join('\n\n') || '(스키마 없음)';
    postMessage({ type: 'ai_chat', service: activeAI, messages: newMessages, context: ctx });
  }

  const connStatusMap = Object.fromEntries(connectors.map((c) => [c.service, c.connected])) as Record<ConnectorService, boolean>;

  return (
    <Flex h="100%" overflow="hidden">

      {/* ── Left sidebar ── */}
      <VStack
        w="220px"
        flexShrink={0}
        borderRight="1px solid var(--border)"
        bg="$sidebarBg"
        overflow="hidden"
      >
        <Box flex={1} overflow="auto" minH={0}>
          <SectionHeader label="EXPORT FILES" />

          <GroupLabel label="MIGRATION SQL" />
          {files.filter((f) => f.id.startsWith('sql-')).map((f) => (
            <FileRow key={f.id} f={f} active={panel.kind === 'file' && panel.id === f.id}
              onClick={() => setPanel({ kind: 'file', id: f.id })} />
          ))}

          <GroupLabel label="SCHEMA" />
          {files.filter((f) => f.id === 'orm-src' || f.id === 'schema-json').map((f) => (
            <FileRow key={f.id} f={f} active={panel.kind === 'file' && panel.id === f.id}
              onClick={() => setPanel({ kind: 'file', id: f.id })} />
          ))}

          <GroupLabel label="DIAGRAM" />
          {files.filter((f) => f.id.startsWith('erd-')).map((f) => (
            <FileRow key={f.id} f={f} active={panel.kind === 'file' && panel.id === f.id}
              onClick={() => setPanel({ kind: 'file', id: f.id })} />
          ))}
        </Box>
      </VStack>

      {/* ── Right panel ── */}
      <VStack flex={1} overflow="hidden">

        {panel.kind === 'file' && selectedFile && (
          <>
            <FileHeader
              file={selectedFile}
              copied={copied === selectedFile.id}
              onCopy={() => copyContent(selectedFile.content, selectedFile.id)}
              onSave={saveFile}
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
    <Box py="8px" px="10px" pb="4px" fontSize="10px" fontWeight={700} letterSpacing="0.08em" color="var(--node-text-dim)" flexShrink={0}>
      {label}
    </Box>
  );
}

function GroupLabel({ label }: { label: string }) {
  return (
    <Box py="6px" px="10px" pb="2px" fontSize="9px" fontWeight={600} color="var(--node-text-dim)" letterSpacing="0.06em">
      {label}
    </Box>
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
      borderLeft={active ? '2px solid var(--focusBorder)' : '2px solid transparent'}
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

function ConnectorRow({ meta, connected, active, onClick }: {
  meta: ConnectorMeta; connected: boolean; active: boolean; onClick: () => void;
}) {
  return (
    <Flex
      onClick={onClick}
      alignItems="center"
      gap="8px"
      py="7px"
      px="10px"
      cursor="pointer"
      bg={active ? 'rgba(99,102,241,0.12)' : 'transparent'}
      borderLeft={active ? '2px solid var(--focusBorder)' : '2px solid transparent'}
    >
      <Flex
        w="22px"
        h="22px"
        borderRadius="5px"
        flexShrink={0}
        bg="$widgetBg"
        border="1px solid var(--node-border)"
        alignItems="center"
        justifyContent="center"
        fontSize="12px"
      >{meta.icon}</Flex>

      <Box as="span" flex={1} fontSize="12px" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap" color="var(--node-text)">
        {meta.label}
      </Box>

      {connected ? (
        <Box as="span" fontSize="10px" color="var(--diff-add-sign)" fontWeight={600} flexShrink={0}>Connected</Box>
      ) : (
        <Box as="span" fontSize="10px" color="var(--node-text-dim)" flexShrink={0}>Connect</Box>
      )}

      <Box as="span" fontSize="9px" color="var(--node-text-dim)" flexShrink={0}>{active ? '▲' : '▼'}</Box>
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
      borderBottom="1px solid var(--border)"
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
          color="var(--editorFg)"
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
          color="var(--editorFg)"
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
          <Box as="span" flex={1} pr="16px" lineHeight="20px" whiteSpace="pre" color="var(--editorFg)">{line}</Box>
        </Flex>
      ))}
    </Box>
  );
}

function ConnectorPanel({ meta, connected, keyValue, saving, onKeyChange, onSave, onDisconnect }: {
  meta: ConnectorMeta; connected: boolean; keyValue: string; saving: boolean;
  onKeyChange: (v: string) => void; onSave: () => void; onDisconnect: () => void;
}) {
  return (
    <Box flex={1} overflow="auto" p="24px" bg="$editorBg">
      <Flex alignItems="center" gap="12px" mb="20px">
        <Flex
          w="40px"
          h="40px"
          borderRadius="10px"
          bg="$widgetBg"
          border="1px solid var(--node-border)"
          alignItems="center"
          justifyContent="center"
          fontSize="20px"
        >{meta.icon}</Flex>
        <Box>
          <Box fontWeight={700} fontSize="14px" color="var(--node-text)">{meta.label}</Box>
          {meta.subtitle && <Box fontSize="11px" color="var(--node-text-dim)" mt="1px">{meta.subtitle}</Box>}
        </Box>
        <Box ml="auto">
          {connected ? (
            <Box as="span" fontSize="12px" color="var(--diff-add-sign)" fontWeight={600}>● Connected</Box>
          ) : (
            <Box as="span" fontSize="12px" color="var(--node-text-dim)">Not connected</Box>
          )}
        </Box>
      </Flex>

      <Box mb="16px">
        <Box as="label" fontSize="11px" color="var(--node-text-dim)" display="block" mb="6px">
          {meta.keyLabel}
        </Box>
        <Box
          as="input"
          type="password"
          value={keyValue}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) => onKeyChange(e.target.value)}
          placeholder={meta.keyPlaceholder}
          onKeyDown={(e: React.KeyboardEvent) => e.key === 'Enter' && onSave()}
          w="100%"
          py="7px"
          px="10px"
          borderRadius="4px"
          fontSize="12px"
          bg="$inputBg"
          border="1px solid var(--inputBorder)"
          color="$fg"
          outline="none"
          boxSizing="border-box"
        />
        {meta.keyHelp && (
          <Box fontSize="10px" color="var(--node-text-dim)" mt="5px">{meta.keyHelp}</Box>
        )}
      </Box>

      <Flex gap="8px">
        <Box as="button" onClick={onSave} disabled={saving || !keyValue.trim()} {...btnStyle('primary')}>
          {saving ? '저장 중...' : connected ? 'Configure' : 'Connect'}
        </Box>
        {connected && (
          <Box as="button" onClick={onDisconnect} {...btnStyle('danger')}>연결 해제</Box>
        )}
      </Flex>
    </Box>
  );
}

function ChatPanel({ messages, loading, input, activeAI, connectedAIs, endRef, onInputChange, onSend, onAIChange }: {
  messages: ChatMessage[]; loading: boolean; input: string; activeAI: ConnectorService;
  connectedAIs: ConnectorMeta[]; endRef: React.RefObject<HTMLDivElement>;
  onInputChange: (v: string) => void; onSend: () => void; onAIChange: (s: ConnectorService) => void;
}) {
  return (
    <VStack flex={1} overflow="hidden" bg="$editorBg">
      <Flex
        py="8px"
        px="12px"
        borderBottom="1px solid var(--border)"
        alignItems="center"
        gap="8px"
        flexShrink={0}
      >
        <Box as="span" fontSize="11px" color="var(--node-text-dim)">AI:</Box>
        {connectedAIs.length === 0 ? (
          <Box as="span" fontSize="11px" color="var(--node-text-dim)">왼쪽에서 AI 서비스를 먼저 연결하세요</Box>
        ) : (
          connectedAIs.map((ai) => (
            <Box
              key={ai.service}
              as="button"
              onClick={() => onAIChange(ai.service)}
              py="2px"
              px="10px"
              borderRadius="12px"
              fontSize="11px"
              cursor="pointer"
              bg={activeAI === ai.service ? 'rgba(99,102,241,0.25)' : 'transparent'}
              border={`1px solid ${activeAI === ai.service ? 'rgba(99,102,241,0.5)' : 'var(--node-border)'}`}
              color={activeAI === ai.service ? '#818cf8' : 'var(--node-text)'}
            >{ai.icon} {ai.label}</Box>
          ))
        )}
      </Flex>

      <Box flex={1} overflow="auto" py="12px" px="16px">
        {messages.length === 0 && (
          <Box color="var(--node-text-dim)" fontSize="12px" lineHeight={1.7} textAlign="center" mt="40px">
            <Box fontSize="24px" mb="8px">🤖</Box>
            <Box>현재 스키마를 컨텍스트로 자동 첨부합니다.</Box>
            <Box mt="8px">
              "Notion에 스키마 정리해줘"<br />
              "Slack에 migration 변경사항 요약 보내줘"<br />
              "이 마이그레이션 검토해줘"
            </Box>
          </Box>
        )}
        {messages.map((m, i) => (
          <ChatBubble key={i} message={m} />
        ))}
        {loading && (
          <Flex gap="6px" py="8px" alignItems="center">
            <Box as="span" fontSize="12px" color="var(--node-text-dim)">응답 생성 중...</Box>
          </Flex>
        )}
        <Box ref={endRef} />
      </Box>

      <Flex
        py="8px"
        px="12px"
        borderTop="1px solid var(--border)"
        gap="8px"
        flexShrink={0}
        bg="$sidebarBg"
      >
        <Box
          as="input"
          value={input}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) => onInputChange(e.target.value)}
          onKeyDown={(e: React.KeyboardEvent) => e.key === 'Enter' && !(e as any).shiftKey && onSend()}
          placeholder="Notion에 스키마 정리해줘..."
          disabled={loading}
          flex={1}
          py="6px"
          px="10px"
          borderRadius="4px"
          fontSize="12px"
          bg="$inputBg"
          border="1px solid var(--inputBorder)"
          color="$fg"
          outline="none"
        />
        <Box as="button" onClick={onSend} disabled={!input.trim() || loading} {...btnStyle('primary')}>전송</Box>
      </Flex>
    </VStack>
  );
}

function ChatBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  return (
    <Flex
      justifyContent={isUser ? 'flex-end' : 'flex-start'}
      mb="10px"
    >
      {!isUser && (
        <Box as="span" fontSize="14px" mr="6px" alignSelf="flex-start" mt="3px" opacity={0.5}>🤖</Box>
      )}
      <Box
        maxW="80%"
        py="8px"
        px="12px"
        borderRadius={isUser ? '12px 12px 2px 12px' : '12px 12px 12px 2px'}
        bg={isUser ? 'rgba(99,102,241,0.15)' : 'var(--widgetBg)'}
        border={`1px solid ${isUser ? 'rgba(99,102,241,0.35)' : 'var(--node-border)'}`}
        fontSize="12px"
        lineHeight={1.7}
        color="var(--node-text)"
        whiteSpace="pre-wrap"
        wordBreak="break-word"
      >
        {message.content}
      </Box>
    </Flex>
  );
}

// ── Style helpers ─────────────────────────────────────────────────────────────

function btnStyle(variant: 'primary' | 'default' | 'green' | 'danger') {
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
  if (variant === 'danger')  return { ...base, bg: 'rgba(239,68,68,0.12)', color: '#f87171', border: '1px solid rgba(239,68,68,0.25)' };
  return { ...base, bg: 'transparent', color: 'var(--node-text)', border: '1px solid var(--node-border)' };
}
