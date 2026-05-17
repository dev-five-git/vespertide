import React, { useState, useEffect, useRef } from 'react';
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

// ── Connector metadata ────────────────────────────────────────────────────────

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

  // Chat state
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
    <div style={{ display: 'flex', height: '100%', overflow: 'hidden' }}>

      {/* ── Left sidebar ── */}
      <div style={{
        width: 220, flexShrink: 0, display: 'flex', flexDirection: 'column',
        borderRight: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
        background: 'var(--vscode-sideBar-background, #252526)',
        overflow: 'hidden',
      }}>
        {/* Export files */}
        <div style={{ flex: 1, overflow: 'auto', minHeight: 0 }}>
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
        </div>

      </div>

      {/* ── Right panel ── */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>

        {/* FILE PREVIEW */}
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

      </div>
    </div>
  );
}

// ── Sub-components ─────────────────────────────────────────────────────────────

function SectionHeader({ label }: { label: string }) {
  return (
    <div style={{ padding: '8px 10px 4px', fontSize: 10, fontWeight: 700, letterSpacing: '0.08em', color: 'var(--node-text-dim)', flexShrink: 0 }}>
      {label}
    </div>
  );
}

function GroupLabel({ label }: { label: string }) {
  return (
    <div style={{ padding: '6px 10px 2px', fontSize: 9, fontWeight: 600, color: 'var(--node-text-dim)', letterSpacing: '0.06em' }}>
      {label}
    </div>
  );
}

function FileRow({ f, active, onClick }: { f: ExportFile; active: boolean; onClick: () => void }) {
  return (
    <div onClick={onClick} style={{
      display: 'flex', alignItems: 'center', gap: 6, padding: '4px 10px',
      cursor: 'pointer',
      background: active ? 'rgba(99,102,241,0.15)' : 'transparent',
      borderLeft: active ? '2px solid var(--vscode-focusBorder, #007acc)' : '2px solid transparent',
    }}>
      <span style={{ fontSize: 10, color: 'var(--node-text-dim)', flexShrink: 0 }}>
        {f.ext === '.sql' ? '≡' : f.ext === '.svg' || f.ext === '.pdf' ? '◫' : '{ }'}
      </span>
      <span style={{ flex: 1, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: 'var(--node-text)' }}>
        {f.label}<span style={{ color: 'var(--node-text-dim)' }}>{f.ext}</span>
      </span>
      {f.isDummy && <span style={{ fontSize: 9, color: 'var(--node-text-dim)' }}>~</span>}
    </div>
  );
}

function ConnectorRow({ meta, connected, active, onClick }: {
  meta: ConnectorMeta; connected: boolean; active: boolean; onClick: () => void;
}) {
  return (
    <div onClick={onClick} style={{
      display: 'flex', alignItems: 'center', gap: 8, padding: '7px 10px',
      cursor: 'pointer',
      background: active ? 'rgba(99,102,241,0.12)' : 'transparent',
      borderLeft: active ? '2px solid var(--vscode-focusBorder, #007acc)' : '2px solid transparent',
    }}>
      {/* Icon */}
      <span style={{
        width: 22, height: 22, borderRadius: 5, flexShrink: 0,
        background: 'var(--vscode-editorWidget-background)', border: '1px solid var(--node-border)',
        display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 12,
      }}>{meta.icon}</span>

      {/* Name */}
      <span style={{ flex: 1, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: 'var(--node-text)' }}>
        {meta.label}
      </span>

      {/* Status */}
      {connected ? (
        <span style={{ fontSize: 10, color: 'var(--diff-add-sign)', fontWeight: 600, flexShrink: 0 }}>Connected</span>
      ) : (
        <span style={{ fontSize: 10, color: 'var(--node-text-dim)', flexShrink: 0 }}>Connect</span>
      )}

      {/* Chevron */}
      <span style={{ fontSize: 9, color: 'var(--node-text-dim)', flexShrink: 0 }}>{active ? '▲' : '▼'}</span>
    </div>
  );
}

function FileHeader({ file, copied, onCopy, onSave }: {
  file: ExportFile; copied: boolean; onCopy: () => void; onSave: () => void;
}) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8, padding: '5px 12px', flexShrink: 0,
      borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
      background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)', fontSize: 12,
    }}>
      <span style={{ fontWeight: 600 }}>{file.label}</span>
      <span style={{ color: 'var(--node-text-dim)' }}>{file.ext}</span>
      <span style={{
        fontSize: 9, padding: '1px 6px', borderRadius: 3,
        background: 'rgba(99,102,241,0.15)', color: '#a5b4fc',
        border: '1px solid rgba(99,102,241,0.25)', fontWeight: 700,
      }}>{file.lang}</span>
      {file.isDummy && (
        <span style={{
          fontSize: 9, padding: '1px 6px', borderRadius: 3,
          background: 'rgba(251,191,36,0.12)', color: '#fbbf24',
          border: '1px solid rgba(251,191,36,0.25)',
        }}>PREVIEW</span>
      )}
      <div style={{ flex: 1 }} />
      <button onClick={onCopy} style={btnStyle(copied ? 'green' : 'default')}>
        {copied ? '✓ 복사됨' : '복사'}
      </button>
      <button onClick={onSave} style={btnStyle('primary')}>저장</button>
    </div>
  );
}

function FilePreviewBody({ file }: { file: ExportFile }) {
  if (file.id === 'erd-pdf') {
    return (
      <div style={{ flex: 1, overflow: 'auto', padding: 24, background: 'var(--vscode-editor-background, #1e1e1e)' }}>
        <div style={{
          padding: '16px 20px', borderRadius: 8,
          background: 'rgba(99,102,241,0.08)', border: '1px solid rgba(99,102,241,0.2)',
          fontSize: 13, lineHeight: 1.8, color: 'var(--vscode-editor-foreground, #d4d4d4)',
        }}>
          PDF export converts the ERD diagram SVG to a portable document.{'\n\n'}
          Click "저장" to generate the PDF file.{'\n'}
          {file.isDummy ? '⚠ ORM Editor에서 스키마를 먼저 입력하세요.' : '✓ ERD 준비 완료.'}
        </div>
      </div>
    );
  }

  if (file.id === 'erd-svg' && file.content.startsWith('<svg')) {
    return (
      <div style={{ flex: 1, overflow: 'auto', padding: 24, background: 'var(--vscode-editor-background, #1e1e1e)' }}>
        <div dangerouslySetInnerHTML={{ __html: file.content }} style={{ maxWidth: '100%' }} />
        <div style={{ marginTop: 16, fontSize: 11, opacity: 0.4 }}>SVG 소스:</div>
        <pre style={{
          marginTop: 8, padding: '10px 14px', background: 'rgba(0,0,0,0.2)',
          borderRadius: 6, fontSize: 11, lineHeight: 1.6, whiteSpace: 'pre',
          overflowX: 'auto', color: 'var(--vscode-editor-foreground, #d4d4d4)',
        }}>{file.content}</pre>
      </div>
    );
  }

  return (
    <div style={{
      flex: 1, overflow: 'auto', background: 'var(--vscode-editor-background, #1e1e1e)',
      fontFamily: 'var(--vscode-editor-font-family, Consolas, "Courier New", monospace)',
      fontSize: 12, lineHeight: '20px',
    }}>
      {file.content.split('\n').map((line, i) => (
        <div key={i} style={{ display: 'flex', minHeight: 20 }}>
          <span style={{
            minWidth: 44, paddingRight: 10, textAlign: 'right', flexShrink: 0,
            fontSize: 11, lineHeight: '20px', userSelect: 'none',
            color: 'var(--diff-linenum)',
          }}>{i + 1}</span>
          <span style={{ flex: 1, paddingRight: 16, lineHeight: '20px', whiteSpace: 'pre', color: 'var(--vscode-editor-foreground, #d4d4d4)' }}>{line}</span>
        </div>
      ))}
    </div>
  );
}

function ConnectorPanel({ meta, connected, keyValue, saving, onKeyChange, onSave, onDisconnect }: {
  meta: ConnectorMeta; connected: boolean; keyValue: string; saving: boolean;
  onKeyChange: (v: string) => void; onSave: () => void; onDisconnect: () => void;
}) {
  return (
    <div style={{ flex: 1, overflow: 'auto', padding: 24, background: 'var(--vscode-editor-background, #1e1e1e)' }}>
      {/* Header row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 20 }}>
        <div style={{
          width: 40, height: 40, borderRadius: 10,
          background: 'var(--vscode-editorWidget-background)', border: '1px solid var(--node-border)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 20,
        }}>{meta.icon}</div>
        <div>
          <div style={{ fontWeight: 700, fontSize: 14, color: 'var(--node-text)' }}>{meta.label}</div>
          {meta.subtitle && <div style={{ fontSize: 11, color: 'var(--node-text-dim)', marginTop: 1 }}>{meta.subtitle}</div>}
        </div>
        <div style={{ marginLeft: 'auto' }}>
          {connected ? (
            <span style={{ fontSize: 12, color: 'var(--diff-add-sign)', fontWeight: 600 }}>● Connected</span>
          ) : (
            <span style={{ fontSize: 12, color: 'var(--node-text-dim)' }}>Not connected</span>
          )}
        </div>
      </div>

      {/* Key input */}
      <div style={{ marginBottom: 16 }}>
        <label style={{ fontSize: 11, color: 'var(--node-text-dim)', display: 'block', marginBottom: 6 }}>
          {meta.keyLabel}
        </label>
        <input
          type="password"
          value={keyValue}
          onChange={(e) => onKeyChange(e.target.value)}
          placeholder={meta.keyPlaceholder}
          onKeyDown={(e) => e.key === 'Enter' && onSave()}
          style={{
            width: '100%', padding: '7px 10px', borderRadius: 4, fontSize: 12,
            background: 'var(--vscode-input-background, rgba(255,255,255,0.06))',
            border: '1px solid var(--vscode-input-border, rgba(255,255,255,0.15))',
            color: 'var(--vscode-foreground, #ccc)', outline: 'none', boxSizing: 'border-box',
          }}
        />
        {meta.keyHelp && (
          <div style={{ fontSize: 10, color: 'var(--node-text-dim)', marginTop: 5 }}>{meta.keyHelp}</div>
        )}
      </div>

      <div style={{ display: 'flex', gap: 8 }}>
        <button onClick={onSave} disabled={saving || !keyValue.trim()} style={btnStyle('primary')}>
          {saving ? '저장 중...' : connected ? 'Configure' : 'Connect'}
        </button>
        {connected && (
          <button onClick={onDisconnect} style={btnStyle('danger')}>연결 해제</button>
        )}
      </div>
    </div>
  );
}

function ChatPanel({ messages, loading, input, activeAI, connectedAIs, endRef, onInputChange, onSend, onAIChange }: {
  messages: ChatMessage[]; loading: boolean; input: string; activeAI: ConnectorService;
  connectedAIs: ConnectorMeta[]; endRef: React.RefObject<HTMLDivElement>;
  onInputChange: (v: string) => void; onSend: () => void; onAIChange: (s: ConnectorService) => void;
}) {
  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden', background: 'var(--vscode-editor-background, #1e1e1e)' }}>
      {/* AI selector */}
      <div style={{
        padding: '8px 12px', borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
        display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0,
      }}>
        <span style={{ fontSize: 11, color: 'var(--node-text-dim)' }}>AI:</span>
        {connectedAIs.length === 0 ? (
          <span style={{ fontSize: 11, color: 'var(--node-text-dim)' }}>왼쪽에서 AI 서비스를 먼저 연결하세요</span>
        ) : (
          connectedAIs.map((ai) => (
            <button key={ai.service} onClick={() => onAIChange(ai.service)} style={{
              padding: '2px 10px', borderRadius: 12, fontSize: 11, cursor: 'pointer',
              background: activeAI === ai.service ? 'rgba(99,102,241,0.25)' : 'transparent',
              border: `1px solid ${activeAI === ai.service ? 'rgba(99,102,241,0.5)' : 'var(--node-border)'}`,
              color: activeAI === ai.service ? '#818cf8' : 'var(--node-text)',
            }}>{ai.icon} {ai.label}</button>
          ))
        )}
      </div>

      {/* Messages */}
      <div style={{ flex: 1, overflow: 'auto', padding: '12px 16px' }}>
        {messages.length === 0 && (
          <div style={{ color: 'var(--node-text-dim)', fontSize: 12, lineHeight: 1.7, textAlign: 'center', marginTop: 40 }}>
            <div style={{ fontSize: 24, marginBottom: 8 }}>🤖</div>
            <div>현재 스키마를 컨텍스트로 자동 첨부합니다.</div>
            <div style={{ marginTop: 8 }}>
              "Notion에 스키마 정리해줘"<br />
              "Slack에 migration 변경사항 요약 보내줘"<br />
              "이 마이그레이션 검토해줘"
            </div>
          </div>
        )}
        {messages.map((m, i) => (
          <ChatBubble key={i} message={m} />
        ))}
        {loading && (
          <div style={{ display: 'flex', gap: 6, padding: '8px 0', alignItems: 'center' }}>
            <span style={{ fontSize: 12, color: 'var(--node-text-dim)' }}>응답 생성 중...</span>
          </div>
        )}
        <div ref={endRef} />
      </div>

      {/* Input */}
      <div style={{
        padding: '8px 12px', borderTop: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
        display: 'flex', gap: 8, flexShrink: 0,
        background: 'var(--vscode-sideBar-background, #252526)',
      }}>
        <input
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && onSend()}
          placeholder="Notion에 스키마 정리해줘..."
          disabled={loading}
          style={{
            flex: 1, padding: '6px 10px', borderRadius: 4, fontSize: 12,
            background: 'var(--vscode-input-background, rgba(255,255,255,0.06))',
            border: '1px solid var(--vscode-input-border, rgba(255,255,255,0.15))',
            color: 'var(--vscode-foreground, #ccc)', outline: 'none',
          }}
        />
        <button onClick={onSend} disabled={!input.trim() || loading}
          style={btnStyle('primary')}>전송</button>
      </div>
    </div>
  );
}

function ChatBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  return (
    <div style={{
      display: 'flex', justifyContent: isUser ? 'flex-end' : 'flex-start',
      marginBottom: 10,
    }}>
      {!isUser && (
        <span style={{ fontSize: 14, marginRight: 6, alignSelf: 'flex-start', marginTop: 3, opacity: 0.5 }}>🤖</span>
      )}
      <div style={{
        maxWidth: '80%', padding: '8px 12px', borderRadius: isUser ? '12px 12px 2px 12px' : '12px 12px 12px 2px',
        background: isUser ? 'rgba(99,102,241,0.15)' : 'var(--vscode-editorWidget-background)',
        border: `1px solid ${isUser ? 'rgba(99,102,241,0.35)' : 'var(--node-border)'}`,
        fontSize: 12, lineHeight: 1.7,
        color: 'var(--node-text)',
        whiteSpace: 'pre-wrap', wordBreak: 'break-word',
      }}>
        {message.content}
      </div>
    </div>
  );
}

// ── Style helpers ─────────────────────────────────────────────────────────────

function btnStyle(variant: 'primary' | 'default' | 'green' | 'danger'): React.CSSProperties {
  const base: React.CSSProperties = {
    border: 'none', borderRadius: 3, padding: '3px 12px',
    fontSize: 11, cursor: 'pointer', fontFamily: 'inherit', flexShrink: 0,
  };
  if (variant === 'primary') return { ...base, background: 'var(--vscode-button-background, #0e639c)', color: 'var(--vscode-button-foreground, #fff)' };
  if (variant === 'green')   return { ...base, background: 'rgba(74,222,128,0.12)', color: '#4ade80', border: '1px solid rgba(74,222,128,0.25)' };
  if (variant === 'danger')  return { ...base, background: 'rgba(239,68,68,0.12)', color: '#f87171', border: '1px solid rgba(239,68,68,0.25)' };
  return { ...base, background: 'transparent', color: 'var(--node-text)', border: '1px solid var(--node-border)' };
}
