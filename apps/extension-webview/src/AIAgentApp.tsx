import React, { useState, useEffect, useRef } from 'react';
import { postMessage, onMessage } from './vscode';
import type { ConnectorService, ConnectorStatus, ChatMessage } from './vscode';

type ConnectorMeta = {
  service: ConnectorService;
  label: string;
  icon: string;
  subtitle: string;
  isAI: boolean;
  keyLabel: string;
  keyPlaceholder: string;
  getKeyUrl: string;
  getKeyLabel: string;
  steps: string[];
};

const CONNECTORS: ConnectorMeta[] = [
  {
    service: 'claude', label: 'Claude', icon: '🤖', subtitle: 'Anthropic', isAI: true,
    keyLabel: 'API Key', keyPlaceholder: 'sk-ant-api03-...',
    getKeyUrl: 'https://console.anthropic.com/settings/keys',
    getKeyLabel: 'API 키 발급 →',
    steps: ['console.anthropic.com 로그인', 'Settings → API Keys', 'Create Key 클릭 후 복사'],
  },
  {
    service: 'openai', label: 'OpenAI / GPT', icon: '🧠', subtitle: 'OpenAI', isAI: true,
    keyLabel: 'API Key', keyPlaceholder: 'sk-proj-...',
    getKeyUrl: 'https://platform.openai.com/api-keys',
    getKeyLabel: 'API 키 발급 →',
    steps: ['platform.openai.com 로그인', 'API Keys 메뉴 선택', 'Create new secret key 클릭 후 복사'],
  },
  {
    service: 'gemini', label: 'Gemini', icon: '✦', subtitle: 'Google', isAI: true,
    keyLabel: 'API Key', keyPlaceholder: 'AIzaSy...',
    getKeyUrl: 'https://aistudio.google.com/app/apikey',
    getKeyLabel: 'API 키 발급 →',
    steps: ['aistudio.google.com 방문', 'Get API Key 클릭', 'Create API key 후 복사'],
  },
  {
    service: 'slack', label: 'Slack', icon: '💬', subtitle: 'Workspace', isAI: false,
    keyLabel: 'Webhook URL', keyPlaceholder: 'https://hooks.slack.com/services/...',
    getKeyUrl: 'https://api.slack.com/apps',
    getKeyLabel: 'Webhook 생성 →',
    steps: ['api.slack.com/apps → Create App', 'Incoming Webhooks 활성화', 'Add New Webhook to Workspace', 'Webhook URL 복사'],
  },
  {
    service: 'notion', label: 'Notion', icon: '📝', subtitle: 'Workspace', isAI: false,
    keyLabel: 'Integration Token', keyPlaceholder: 'secret_...',
    getKeyUrl: 'https://www.notion.so/my-integrations',
    getKeyLabel: 'Integration 생성 →',
    steps: ['notion.so/my-integrations 방문', 'New integration 생성', 'Internal Integration Token 복사'],
  },
  {
    service: 'jira', label: 'Jira', icon: '🎯', subtitle: 'Atlassian', isAI: false,
    keyLabel: 'Email:API Token', keyPlaceholder: 'user@example.com:ATATT...',
    getKeyUrl: 'https://id.atlassian.com/manage-profile/security/api-tokens',
    getKeyLabel: 'API 토큰 발급 →',
    steps: ['Atlassian 계정 로그인', 'API tokens → Create API token', '"이메일:토큰" 형식으로 입력\n예) user@example.com:ATATT3xFfGF0...'],
  },
];

type View = 'chat' | 'connectors';

export default function AIAgentApp() {
  const [theme, setTheme]               = useState<'dark' | 'light'>('dark');
  const [view, setView]                 = useState<View>('chat');
  const [connectors, setConnectors]     = useState<ConnectorStatus[]>([]);
  const [messages, setMessages]         = useState<ChatMessage[]>([]);
  const [input, setInput]               = useState('');
  const [loading, setLoading]           = useState(false);
  const [activeAI, setActiveAI]         = useState<ConnectorService>('claude');
  const [configuring, setConfiguring]   = useState<ConnectorService | null>(null);
  const [keyInputs, setKeyInputs]       = useState<Partial<Record<ConnectorService, string>>>({});
  const [saving, setSaving]             = useState<ConnectorService | null>(null);
  const [showKey, setShowKey]           = useState<Partial<Record<ConnectorService, boolean>>>({});
  const endRef = useRef<HTMLDivElement>(null) as React.RefObject<HTMLDivElement>;

  const statusMap = Object.fromEntries(
    connectors.map((c) => [c.service, c.connected])
  ) as Record<ConnectorService, boolean>;

  const connectedAIs = CONNECTORS.filter((c) => c.isAI && statusMap[c.service]);

  useEffect(() => { postMessage({ type: 'connector_load' }); }, []);

  useEffect(() => {
    return onMessage((msg) => {
      if (msg.type === 'connector_status') setConnectors(msg.connectors);
      if (msg.type === 'ai_response' && msg.done) {
        setMessages((prev) => [...prev, { role: 'assistant', content: msg.content }]);
        setLoading(false);
      }
      if (msg.type === 'error') {
        setMessages((prev) => [...prev, { role: 'assistant', content: `오류: ${msg.message}` }]);
        setLoading(false);
      }
    });
  }, []);

  useEffect(() => { endRef.current?.scrollIntoView({ behavior: 'smooth' }); }, [messages]);

  function openBrowser(url: string) { postMessage({ type: 'open_external', url }); }

  function saveKey(service: ConnectorService) {
    const key = keyInputs[service]?.trim();
    if (!key) return;
    setSaving(service);
    postMessage({ type: 'connector_save', service, key });
    setKeyInputs((prev) => ({ ...prev, [service]: '' }));
    setTimeout(() => { setSaving(null); setConfiguring(null); }, 1200);
  }

  function disconnect(service: ConnectorService) {
    postMessage({ type: 'connector_delete', service });
    if (activeAI === service) setActiveAI('claude');
  }

  function sendChat() {
    const text = input.trim();
    if (!text || loading) return;
    setInput('');
    if (connectedAIs.length === 0) {
      setMessages((prev) => [
        ...prev,
        { role: 'user', content: text },
        { role: 'assistant', content: '오른쪽 Connectors 탭에서 AI 서비스를 먼저 연결해주세요.' },
      ]);
      return;
    }
    const newMessages: ChatMessage[] = [...messages, { role: 'user', content: text }];
    setMessages(newMessages);
    setLoading(true);
    postMessage({ type: 'ai_chat', service: activeAI, messages: newMessages, context: '' });
  }

  return (
    <div
      data-theme={theme}
      style={{
        display: 'flex', flexDirection: 'column', height: '100vh',
        background: 'var(--vscode-editor-background, #1e1e1e)',
        color: 'var(--vscode-foreground, #ccc)',
        fontFamily: 'var(--vscode-font-family, sans-serif)',
      }}
    >
      {/* ── Header / Tab bar ── */}
      <div style={{
        display: 'flex', alignItems: 'stretch', flexShrink: 0,
        borderBottom: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
        background: 'var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '0 14px', flexShrink: 0 }}>
          <span style={{ fontSize: 15 }}>🤖</span>
          <span style={{ fontWeight: 700, fontSize: 13, color: 'var(--node-text)' }}>AI Agent</span>
        </div>

        <div style={{ flex: 1, display: 'flex' }}>
          {(['chat', 'connectors'] as View[]).map((v) => (
            <button key={v} onClick={() => setView(v)} style={{
              padding: '8px 16px', border: 'none', cursor: 'pointer',
              background: 'transparent',
              borderBottom: view === v
                ? '2px solid var(--vscode-focusBorder, #007acc)'
                : '2px solid transparent',
              color: view === v ? 'var(--node-text)' : 'var(--node-text-dim)',
              fontSize: 12, fontWeight: view === v ? 600 : 400,
            }}>
              {v === 'chat' ? 'Chat' : 'Connectors'}
              {v === 'connectors' && connectedAIs.length > 0 && (
                <span style={{
                  marginLeft: 6, fontSize: 10, padding: '1px 5px', borderRadius: 8,
                  background: 'rgba(74,222,128,0.18)', color: 'var(--diff-add-sign)',
                }}>{connectedAIs.length}</span>
              )}
            </button>
          ))}
        </div>

        {view === 'chat' && connectedAIs.length > 0 && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '0 10px' }}>
            {connectedAIs.map((ai) => (
              <button key={ai.service} onClick={() => setActiveAI(ai.service)} style={{
                padding: '3px 10px', borderRadius: 10, fontSize: 11, cursor: 'pointer',
                background: activeAI === ai.service ? 'rgba(99,102,241,0.28)' : 'transparent',
                border: `1px solid ${activeAI === ai.service ? 'rgba(99,102,241,0.6)' : 'var(--node-border)'}`,
                color: activeAI === ai.service ? '#818cf8' : 'var(--node-text-dim)',
              }}>
                {ai.icon} {ai.label}
              </button>
            ))}
          </div>
        )}

        <button
          onClick={() => setTheme((t) => t === 'dark' ? 'light' : 'dark')}
          title={theme === 'dark' ? '라이트 모드로 전환' : '다크 모드로 전환'}
          style={{
            width: 36, flexShrink: 0, border: 'none',
            borderBottom: '2px solid transparent',
            background: 'transparent', color: 'var(--node-text-dim)',
            cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center',
          }}
        >
          {theme === 'dark'
            ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>
            : <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
          }
        </button>
      </div>

      {/* ── Chat view ── */}
      {view === 'chat' && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={{ flex: 1, overflow: 'auto', padding: '16px 20px' }}>
            {messages.length === 0 && (
              <div style={{ color: 'var(--node-text-dim)', fontSize: 13, textAlign: 'center', marginTop: 60, lineHeight: 1.9 }}>
                <div style={{ fontSize: 36, marginBottom: 10 }}>🤖</div>
                <div style={{ fontWeight: 600, color: 'var(--node-text)' }}>Vespertide AI Agent</div>
                {connectedAIs.length === 0 ? (
                  <div style={{ marginTop: 12, fontSize: 12 }}>
                    <span style={{ color: 'var(--node-text-dim)' }}>AI가 연결되지 않았습니다.</span>
                    <br />
                    <button onClick={() => setView('connectors')} style={{
                      marginTop: 10, padding: '6px 14px', borderRadius: 6, border: 'none',
                      background: 'rgba(99,102,241,0.2)', color: '#818cf8',
                      cursor: 'pointer', fontSize: 12,
                    }}>
                      Connectors에서 AI 추가하기 →
                    </button>
                  </div>
                ) : (
                  <div style={{ marginTop: 8, fontSize: 12, color: 'var(--node-text-dim)' }}>
                    "이 스키마를 Notion에 정리해줘"<br />
                    "마이그레이션 변경사항을 Slack으로 보내줘"<br />
                    "Post 테이블에 인덱스를 추가하면 좋을까?"
                  </div>
                )}
              </div>
            )}

            {messages.map((m, i) => {
              const isUser = m.role === 'user';
              return (
                <div key={i} style={{ display: 'flex', justifyContent: isUser ? 'flex-end' : 'flex-start', marginBottom: 12 }}>
                  {!isUser && (
                    <span style={{ fontSize: 16, marginRight: 8, alignSelf: 'flex-start', marginTop: 4, color: 'var(--node-text-dim)' }}>🤖</span>
                  )}
                  <div style={{
                    maxWidth: '72%', padding: '10px 14px',
                    borderRadius: isUser ? '14px 14px 3px 14px' : '14px 14px 14px 3px',
                    background: theme === 'light'
                      ? '#ffffff'
                      : isUser ? 'rgba(99,102,241,0.15)' : 'var(--vscode-editorWidget-background, rgba(255,255,255,0.05))',
                    border: theme === 'light'
                      ? '1px solid rgba(0,0,0,0.18)'
                      : `1px solid ${isUser ? 'rgba(99,102,241,0.4)' : 'var(--node-border)'}`,
                    fontSize: 13, lineHeight: 1.7, color: 'var(--node-text)',
                    whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                  }}>
                    {m.content}
                  </div>
                </div>
              );
            })}

            {loading && (
              <div style={{ display: 'flex', gap: 8, padding: '8px 0', alignItems: 'center' }}>
                <span style={{ fontSize: 16, color: 'var(--node-text-dim)' }}>🤖</span>
                <span style={{ fontSize: 12, color: 'var(--node-text-dim)' }}>응답 생성 중...</span>
              </div>
            )}
            <div ref={endRef} />
          </div>

          <div style={{
            padding: '10px 16px', flexShrink: 0,
            borderTop: '1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))',
            background: 'var(--vscode-sideBar-background, #252526)',
            display: 'flex', gap: 8, alignItems: 'flex-end',
          }}>
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChat(); } }}
              placeholder="질문하세요... (Shift+Enter: 줄바꿈)"
              disabled={loading}
              rows={1}
              style={{
                flex: 1, padding: '8px 12px', borderRadius: 6, fontSize: 12,
                background: theme === 'light' ? '#ffffff' : 'var(--vscode-input-background, rgba(255,255,255,0.06))',
                border: `1px solid ${theme === 'light' ? 'rgba(0,0,0,0.2)' : 'var(--vscode-input-border, rgba(255,255,255,0.15))'}`,
                color: 'var(--node-text)', outline: 'none',
                resize: 'none', lineHeight: 1.6, fontFamily: 'inherit', maxHeight: 120, overflowY: 'auto',
              }}
            />
            <button onClick={sendChat} disabled={!input.trim() || loading} style={{
              padding: '8px 16px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12, fontWeight: 600,
              background: !input.trim() || loading ? 'rgba(99,102,241,0.15)' : 'var(--vscode-button-background, #0e639c)',
              color: !input.trim() || loading ? 'var(--node-text-dim)' : 'var(--vscode-button-foreground, #fff)',
            }}>전송</button>
          </div>
        </div>
      )}

      {/* ── Connectors view ── */}
      {view === 'connectors' && (
        <div style={{ flex: 1, overflow: 'auto' }}>
          <SectionLabel label="AI" />
          {CONNECTORS.filter((c) => c.isAI).map((meta) => (
            <ConnectorItem
              key={meta.service}
              meta={meta}
              theme={theme}
              connected={!!statusMap[meta.service]}
              isActive={activeAI === meta.service && !!statusMap[meta.service]}
              configuring={configuring === meta.service}
              keyValue={keyInputs[meta.service] ?? ''}
              saving={saving === meta.service}
              showKey={!!showKey[meta.service]}
              onSetActive={() => { setActiveAI(meta.service); setView('chat'); }}
              onConfigure={() => setConfiguring(configuring === meta.service ? null : meta.service)}
              onKeyChange={(v) => setKeyInputs((p) => ({ ...p, [meta.service]: v }))}
              onToggleShow={() => setShowKey((p) => ({ ...p, [meta.service]: !p[meta.service] }))}
              onSave={() => saveKey(meta.service)}
              onDisconnect={() => disconnect(meta.service)}
              onOpenBrowser={() => openBrowser(meta.getKeyUrl)}
            />
          ))}

          <SectionLabel label="기타 연동" style={{ marginTop: 8 }} />
          {CONNECTORS.filter((c) => !c.isAI).map((meta) => (
            <ConnectorItem
              key={meta.service}
              meta={meta}
              theme={theme}
              connected={!!statusMap[meta.service]}
              isActive={false}
              configuring={configuring === meta.service}
              keyValue={keyInputs[meta.service] ?? ''}
              saving={saving === meta.service}
              showKey={!!showKey[meta.service]}
              onSetActive={() => {}}
              onConfigure={() => setConfiguring(configuring === meta.service ? null : meta.service)}
              onKeyChange={(v) => setKeyInputs((p) => ({ ...p, [meta.service]: v }))}
              onToggleShow={() => setShowKey((p) => ({ ...p, [meta.service]: !p[meta.service] }))}
              onSave={() => saveKey(meta.service)}
              onDisconnect={() => disconnect(meta.service)}
              onOpenBrowser={() => openBrowser(meta.getKeyUrl)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Sub-components ────────────────────────────────────────────────────────────

function SectionLabel({ label, style }: { label: string; style?: React.CSSProperties }) {
  return (
    <div style={{
      padding: '10px 18px 4px', fontSize: 10, fontWeight: 700,
      letterSpacing: '0.08em', color: 'var(--node-text-dim)',
      ...style,
    }}>
      {label}
    </div>
  );
}

function ConnectorItem({
  meta, connected, isActive, configuring,
  keyValue, saving, showKey, theme,
  onSetActive, onConfigure, onKeyChange, onToggleShow, onSave, onDisconnect, onOpenBrowser,
}: {
  meta: ConnectorMeta; connected: boolean; isActive: boolean; configuring: boolean;
  keyValue: string; saving: boolean; showKey: boolean; theme: 'dark' | 'light';
  onSetActive: () => void; onConfigure: () => void;
  onKeyChange: (v: string) => void; onToggleShow: () => void;
  onSave: () => void; onDisconnect: () => void; onOpenBrowser: () => void;
}) {
  return (
    <div style={{ borderBottom: '1px solid var(--node-field-divider)' }}>
      {/* Row */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 12, padding: '12px 18px',
        background: configuring ? 'rgba(99,102,241,0.06)' : 'transparent',
      }}>
        <div style={{
          width: 36, height: 36, borderRadius: 8, flexShrink: 0,
          background: 'var(--vscode-editorWidget-background, rgba(255,255,255,0.05))',
          border: '1px solid var(--node-border)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 18,
        }}>{meta.icon}</div>

        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ fontWeight: 600, fontSize: 13, color: 'var(--node-text)' }}>{meta.label}</span>
            {isActive && (
              <span style={{
                fontSize: 9, padding: '1px 6px', borderRadius: 6, fontWeight: 700,
                background: 'rgba(99,102,241,0.2)', color: '#818cf8',
                border: '1px solid rgba(99,102,241,0.4)',
              }}>사용 중</span>
            )}
          </div>
          <div style={{ fontSize: 11, marginTop: 1 }}>
            {connected
              ? <span style={{ color: 'var(--diff-add-sign)' }}>● 연결됨</span>
              : <span style={{ color: 'var(--node-text-dim)' }}>{meta.subtitle}</span>
            }
          </div>
        </div>

        <div style={{ display: 'flex', gap: 6, flexShrink: 0 }}>
          {connected && meta.isAI && !isActive && (
            <button onClick={onSetActive} style={btn('ghost')}>사용</button>
          )}
          {connected ? (
            <button onClick={onConfigure} style={btn(configuring ? 'active' : 'ghost')}>
              {configuring ? '닫기' : '설정'}
            </button>
          ) : (
            <button onClick={onConfigure} style={btn('primary')}>
              {configuring ? '닫기' : '+ 연결'}
            </button>
          )}
        </div>
      </div>

      {/* Config panel */}
      {configuring && (
        <div style={{
          padding: '0 18px 16px',
          background: 'var(--vscode-sideBar-background, rgba(0,0,0,0.15))',
          borderTop: '1px solid var(--node-field-divider)',
        }}>
          {/* Step-by-step guide */}
          <div style={{
            marginTop: 12, marginBottom: 12, padding: '10px 12px', borderRadius: 6,
            background: theme === 'light' ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.04)',
            border: '1px solid var(--node-field-divider)',
          }}>
            <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--node-text-dim)', marginBottom: 6 }}>
              키 발급 방법
            </div>
            {meta.steps.map((step, i) => (
              <div key={i} style={{ display: 'flex', gap: 8, marginBottom: i < meta.steps.length - 1 ? 4 : 0 }}>
                <span style={{
                  fontSize: 10, fontWeight: 700, color: '#818cf8',
                  minWidth: 16, marginTop: 1,
                }}>{i + 1}.</span>
                <span style={{ fontSize: 11, color: 'var(--node-text)', lineHeight: 1.5, whiteSpace: 'pre-wrap' }}>{step}</span>
              </div>
            ))}
            <button onClick={onOpenBrowser} style={{
              display: 'inline-flex', alignItems: 'center', gap: 4, marginTop: 8,
              padding: '4px 10px', borderRadius: 4, border: 'none',
              background: 'rgba(99,102,241,0.15)', color: '#818cf8',
              fontSize: 11, cursor: 'pointer', fontFamily: 'inherit',
            }}>
              🔗 {meta.getKeyLabel}
            </button>
          </div>

          <label style={{ fontSize: 11, color: 'var(--node-text-dim)', display: 'block', marginBottom: 5 }}>
            {meta.keyLabel}
          </label>

          <div style={{ display: 'flex', gap: 6 }}>
            <input
              type={showKey ? 'text' : 'password'}
              value={keyValue}
              onChange={(e) => onKeyChange(e.target.value)}
              placeholder={meta.keyPlaceholder}
              onKeyDown={(e) => e.key === 'Enter' && onSave()}
              style={{
                flex: 1, padding: '7px 10px', borderRadius: 4, fontSize: 12,
                background: theme === 'light' ? '#ffffff' : 'var(--vscode-input-background, rgba(255,255,255,0.07))',
                border: `1px solid ${theme === 'light' ? 'rgba(0,0,0,0.2)' : 'var(--vscode-input-border, rgba(255,255,255,0.15))'}`,
                color: 'var(--node-text)', outline: 'none',
              }}
            />
            <button onClick={onToggleShow} style={{ ...btn('ghost'), padding: '0 8px' }}>
              {showKey ? '숨김' : '표시'}
            </button>
          </div>

          <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
            <button onClick={onSave} disabled={saving || !keyValue.trim()} style={btn('primary')}>
              {saving ? '저장 중...' : connected ? 'Update' : 'Connect'}
            </button>
            {connected && (
              <button onClick={onDisconnect} style={btn('danger')}>연결 해제</button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function btn(variant: 'primary' | 'ghost' | 'active' | 'danger'): React.CSSProperties {
  const base: React.CSSProperties = {
    padding: '4px 12px', borderRadius: 4, fontSize: 11,
    cursor: 'pointer', fontFamily: 'inherit',
  };
  if (variant === 'primary') return { ...base, background: 'var(--vscode-button-background, #0e639c)', color: 'var(--vscode-button-foreground, #fff)', border: 'none' };
  if (variant === 'ghost')   return { ...base, background: 'transparent', color: 'var(--node-text)', border: '1px solid var(--node-border)' };
  if (variant === 'active')  return { ...base, background: 'rgba(99,102,241,0.2)', color: '#818cf8', border: '1px solid rgba(99,102,241,0.4)' };
  if (variant === 'danger')  return { ...base, background: 'rgba(239,68,68,0.12)', color: 'var(--diff-rm-sign)', border: '1px solid rgba(239,68,68,0.2)' };
  return base;
}
