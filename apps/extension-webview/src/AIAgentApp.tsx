import React, { useState, useEffect, useRef } from 'react';
import { Box, Flex } from '@devup-ui/react';
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
  const [theme, setTheme]             = useState<'dark' | 'light'>('dark');
  const [view, setView]               = useState<View>('chat');
  const [connectors, setConnectors]   = useState<ConnectorStatus[]>([]);
  const [messages, setMessages]       = useState<ChatMessage[]>([]);
  const [input, setInput]             = useState('');
  const [loading, setLoading]         = useState(false);
  const [activeAI, setActiveAI]       = useState<ConnectorService>('claude');
  const [configuring, setConfiguring] = useState<ConnectorService | null>(null);
  const [keyInputs, setKeyInputs]     = useState<Partial<Record<ConnectorService, string>>>({});
  const [saving, setSaving]           = useState<ConnectorService | null>(null);
  const [showKey, setShowKey]         = useState<Partial<Record<ConnectorService, boolean>>>({});
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
    <Box
      data-theme={theme}
      display="flex"
      flexDir="column"
      h="100vh"
      bg="var(--vscode-editor-background, #1e1e1e)"
      color="var(--vscode-foreground, #ccc)"
      fontFamily="var(--vscode-font-family, sans-serif)"
    >
      {/* ── Header / Tab bar ── */}
      <Flex
        alignItems="stretch"
        flexShrink={0}
        borderBottom="1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))"
        bg="var(--vscode-editorGroupHeader-tabsBackground, #2d2d2d)"
      >
        <Flex alignItems="center" gap="6px" px="14px" flexShrink={0}>
          <Box as="span" fontSize="15px">🤖</Box>
          <Box as="span" fontWeight={700} fontSize="13px" color="var(--node-text)">AI Agent</Box>
        </Flex>

        <Flex flex={1}>
          {(['chat', 'connectors'] as View[]).map((v) => (
            <Box
              key={v}
              as="button"
              onClick={() => setView(v)}
              py="8px"
              px="16px"
              border="none"
              cursor="pointer"
              bg="transparent"
              borderBottom={view === v ? '2px solid var(--vscode-focusBorder, #007acc)' : '2px solid transparent'}
              color={view === v ? 'var(--node-text)' : 'var(--node-text-dim)'}
              fontSize="12px"
              fontWeight={view === v ? 600 : 400}
            >
              {v === 'chat' ? 'Chat' : 'Connectors'}
              {v === 'connectors' && connectedAIs.length > 0 && (
                <Box
                  as="span"
                  ml="6px"
                  fontSize="10px"
                  px="5px"
                  py="1px"
                  borderRadius="8px"
                  bg="rgba(74,222,128,0.18)"
                  color="var(--diff-add-sign)"
                >{connectedAIs.length}</Box>
              )}
            </Box>
          ))}
        </Flex>

        {view === 'chat' && connectedAIs.length > 0 && (
          <Flex alignItems="center" gap="4px" px="10px">
            {connectedAIs.map((ai) => (
              <Box
                key={ai.service}
                as="button"
                onClick={() => setActiveAI(ai.service)}
                py="3px"
                px="10px"
                borderRadius="10px"
                fontSize="11px"
                cursor="pointer"
                bg={activeAI === ai.service ? 'rgba(99,102,241,0.28)' : 'transparent'}
                border={`1px solid ${activeAI === ai.service ? 'rgba(99,102,241,0.6)' : 'var(--node-border)'}`}
                color={activeAI === ai.service ? '#818cf8' : 'var(--node-text-dim)'}
              >
                {ai.icon} {ai.label}
              </Box>
            ))}
          </Flex>
        )}

        <Box
          as="button"
          onClick={() => setTheme((t) => t === 'dark' ? 'light' : 'dark')}
          title={theme === 'dark' ? '라이트 모드로 전환' : '다크 모드로 전환'}
          w="36px"
          flexShrink={0}
          border="none"
          borderBottom="2px solid transparent"
          bg="transparent"
          color="var(--node-text-dim)"
          cursor="pointer"
          display="flex"
          alignItems="center"
          justifyContent="center"
        >
          {theme === 'dark'
            ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>
            : <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
          }
        </Box>
      </Flex>

      {/* ── Chat view ── */}
      {view === 'chat' && (
        <Flex flex={1} flexDir="column" overflow="hidden">
          <Box flex={1} overflow="auto" px="20px" py="16px">
            {messages.length === 0 && (
              <Flex flexDir="column" alignItems="center" color="var(--node-text-dim)" fontSize="13px" textAlign="center" mt="60px" lineHeight={1.9}>
                <Box fontSize="36px" mb="10px">🤖</Box>
                <Box fontWeight={600} color="var(--node-text)">Vespertide AI Agent</Box>
                {connectedAIs.length === 0 ? (
                  <Box mt="12px" fontSize="12px">
                    <Box as="span" color="var(--node-text-dim)">AI가 연결되지 않았습니다.</Box>
                    <br />
                    <Box
                      as="button"
                      onClick={() => setView('connectors')}
                      mt="10px"
                      py="6px"
                      px="14px"
                      borderRadius="6px"
                      border="none"
                      bg="rgba(99,102,241,0.2)"
                      color="#818cf8"
                      cursor="pointer"
                      fontSize="12px"
                    >
                      Connectors에서 AI 추가하기 →
                    </Box>
                  </Box>
                ) : (
                  <Box mt="8px" fontSize="12px" color="var(--node-text-dim)">
                    "이 스키마를 Notion에 정리해줘"<br />
                    "마이그레이션 변경사항을 Slack으로 보내줘"<br />
                    "Post 테이블에 인덱스를 추가하면 좋을까?"
                  </Box>
                )}
              </Flex>
            )}

            {messages.map((m, i) => {
              const isUser = m.role === 'user';
              return (
                <Flex key={i} justifyContent={isUser ? 'flex-end' : 'flex-start'} mb="12px">
                  {!isUser && (
                    <Box as="span" fontSize="16px" mr="8px" alignSelf="flex-start" mt="4px" color="var(--node-text-dim)">🤖</Box>
                  )}
                  <Box
                    maxWidth="72%"
                    py="10px"
                    px="14px"
                    borderRadius={isUser ? '14px 14px 3px 14px' : '14px 14px 14px 3px'}
                    bg={theme === 'light' ? '#ffffff' : isUser ? 'rgba(99,102,241,0.15)' : 'var(--vscode-editorWidget-background, rgba(255,255,255,0.05))'}
                    border={theme === 'light' ? '1px solid rgba(0,0,0,0.18)' : `1px solid ${isUser ? 'rgba(99,102,241,0.4)' : 'var(--node-border)'}`}
                    fontSize="13px"
                    lineHeight={1.7}
                    color="var(--node-text)"
                    whiteSpace="pre-wrap"
                    wordBreak="break-word"
                  >
                    {m.content}
                  </Box>
                </Flex>
              );
            })}

            {loading && (
              <Flex gap="8px" py="8px" alignItems="center">
                <Box as="span" fontSize="16px" color="var(--node-text-dim)">🤖</Box>
                <Box as="span" fontSize="12px" color="var(--node-text-dim)">응답 생성 중...</Box>
              </Flex>
            )}
            <Box ref={endRef} />
          </Box>

          <Flex
            py="10px"
            px="16px"
            flexShrink={0}
            borderTop="1px solid var(--vscode-panel-border, rgba(255,255,255,0.1))"
            bg="var(--vscode-sideBar-background, #252526)"
            gap="8px"
            alignItems="flex-end"
          >
            <Box
              as="textarea"
              value={input}
              onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) => setInput(e.target.value)}
              onKeyDown={(e: React.KeyboardEvent<HTMLTextAreaElement>) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChat(); } }}
              placeholder="질문하세요... (Shift+Enter: 줄바꿈)"
              disabled={loading}
              rows={1}
              flex={1}
              py="8px"
              px="12px"
              borderRadius="6px"
              fontSize="12px"
              bg={theme === 'light' ? '#ffffff' : 'var(--vscode-input-background, rgba(255,255,255,0.06))'}
              border={`1px solid ${theme === 'light' ? 'rgba(0,0,0,0.2)' : 'var(--vscode-input-border, rgba(255,255,255,0.15))'}`}
              color="var(--node-text)"
              outline="none"
              resize="none"
              lineHeight={1.6}
              fontFamily="inherit"
              maxHeight="120px"
              overflowY="auto"
            />
            <Box
              as="button"
              onClick={sendChat}
              disabled={!input.trim() || loading}
              py="8px"
              px="16px"
              borderRadius="6px"
              border="none"
              cursor="pointer"
              fontSize="12px"
              fontWeight={600}
              bg={!input.trim() || loading ? 'rgba(99,102,241,0.15)' : 'var(--vscode-button-background, #0e639c)'}
              color={!input.trim() || loading ? 'var(--node-text-dim)' : 'var(--vscode-button-foreground, #fff)'}
            >전송</Box>
          </Flex>
        </Flex>
      )}

      {/* ── Connectors view ── */}
      {view === 'connectors' && (
        <Box flex={1} overflow="auto">
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

          <SectionLabel label="기타 연동" mt="8px" />
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
        </Box>
      )}
    </Box>
  );
}

// ── Sub-components ────────────────────────────────────────────────────────────

function SectionLabel({ label, mt }: { label: string; mt?: string }) {
  return (
    <Box
      pt="10px"
      pb="4px"
      px="18px"
      fontSize="10px"
      fontWeight={700}
      letterSpacing="0.08em"
      color="var(--node-text-dim)"
      mt={mt}
    >
      {label}
    </Box>
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
    <Box borderBottom="1px solid var(--node-field-divider)">
      <Flex
        alignItems="center"
        gap="12px"
        py="12px"
        px="18px"
        bg={configuring ? 'rgba(99,102,241,0.06)' : 'transparent'}
      >
        <Flex
          w="36px"
          h="36px"
          borderRadius="8px"
          flexShrink={0}
          bg="var(--vscode-editorWidget-background, rgba(255,255,255,0.05))"
          border="1px solid var(--node-border)"
          alignItems="center"
          justifyContent="center"
          fontSize="18px"
        >{meta.icon}</Flex>

        <Box flex={1} minWidth={0}>
          <Flex alignItems="center" gap="6px">
            <Box as="span" fontWeight={600} fontSize="13px" color="var(--node-text)">{meta.label}</Box>
            {isActive && (
              <Box
                as="span"
                fontSize="9px"
                py="1px"
                px="6px"
                borderRadius="6px"
                fontWeight={700}
                bg="rgba(99,102,241,0.2)"
                color="#818cf8"
                border="1px solid rgba(99,102,241,0.4)"
              >사용 중</Box>
            )}
          </Flex>
          <Box fontSize="11px" mt="1px">
            {connected
              ? <Box as="span" color="var(--diff-add-sign)">● 연결됨</Box>
              : <Box as="span" color="var(--node-text-dim)">{meta.subtitle}</Box>
            }
          </Box>
        </Box>

        <Flex gap="6px" flexShrink={0}>
          {connected && meta.isAI && !isActive && (
            <BtnBox variant="ghost" onClick={onSetActive}>사용</BtnBox>
          )}
          {connected ? (
            <BtnBox variant={configuring ? 'active' : 'ghost'} onClick={onConfigure}>
              {configuring ? '닫기' : '설정'}
            </BtnBox>
          ) : (
            <BtnBox variant="primary" onClick={onConfigure}>
              {configuring ? '닫기' : '+ 연결'}
            </BtnBox>
          )}
        </Flex>
      </Flex>

      {configuring && (
        <Box
          py="16px"
          px="18px"
          bg="var(--vscode-sideBar-background, rgba(0,0,0,0.15))"
          borderTop="1px solid var(--node-field-divider)"
        >
          <Box
            mt="12px"
            mb="12px"
            py="10px"
            px="12px"
            borderRadius="6px"
            bg={theme === 'light' ? 'rgba(0,0,0,0.04)' : 'rgba(255,255,255,0.04)'}
            border="1px solid var(--node-field-divider)"
          >
            <Box fontSize="11px" fontWeight={600} color="var(--node-text-dim)" mb="6px">
              키 발급 방법
            </Box>
            {meta.steps.map((step, i) => (
              <Flex key={i} gap="8px" mb={i < meta.steps.length - 1 ? '4px' : '0'}>
                <Box as="span" fontSize="10px" fontWeight={700} color="#818cf8" minWidth="16px" mt="1px">{i + 1}.</Box>
                <Box as="span" fontSize="11px" color="var(--node-text)" lineHeight={1.5} whiteSpace="pre-wrap">{step}</Box>
              </Flex>
            ))}
            <Flex
              as="button"
              alignItems="center"
              gap="4px"
              mt="8px"
              py="4px"
              px="10px"
              borderRadius="4px"
              border="none"
              bg="rgba(99,102,241,0.15)"
              color="#818cf8"
              fontSize="11px"
              cursor="pointer"
              fontFamily="inherit"
              onClick={onOpenBrowser}
            >
              🔗 {meta.getKeyLabel}
            </Flex>
          </Box>

          <Box as="label" fontSize="11px" color="var(--node-text-dim)" display="block" mb="5px">
            {meta.keyLabel}
          </Box>

          <Flex gap="6px">
            <Box
              as="input"
              type={showKey ? 'text' : 'password'}
              value={keyValue}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => onKeyChange(e.target.value)}
              placeholder={meta.keyPlaceholder}
              onKeyDown={(e: React.KeyboardEvent<HTMLInputElement>) => e.key === 'Enter' && onSave()}
              flex={1}
              py="7px"
              px="10px"
              borderRadius="4px"
              fontSize="12px"
              bg={theme === 'light' ? '#ffffff' : 'var(--vscode-input-background, rgba(255,255,255,0.07))'}
              border={`1px solid ${theme === 'light' ? 'rgba(0,0,0,0.2)' : 'var(--vscode-input-border, rgba(255,255,255,0.15))'}`}
              color="var(--node-text)"
              outline="none"
            />
            <BtnBox variant="ghost" onClick={onToggleShow} px="8px">
              {showKey ? '숨김' : '표시'}
            </BtnBox>
          </Flex>

          <Flex gap="6px" mt="8px">
            <BtnBox variant="primary" onClick={onSave} disabled={saving || !keyValue.trim()}>
              {saving ? '저장 중...' : connected ? 'Update' : 'Connect'}
            </BtnBox>
            {connected && (
              <BtnBox variant="danger" onClick={onDisconnect}>연결 해제</BtnBox>
            )}
          </Flex>
        </Box>
      )}
    </Box>
  );
}

function BtnBox({
  variant, children, onClick, disabled, px,
}: {
  variant: 'primary' | 'ghost' | 'active' | 'danger';
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  px?: string;
}) {
  const styles = {
    primary: { bg: 'var(--vscode-button-background, #0e639c)', color: 'var(--vscode-button-foreground, #fff)', border: 'none' },
    ghost:   { bg: 'transparent', color: 'var(--node-text)', border: '1px solid var(--node-border)' },
    active:  { bg: 'rgba(99,102,241,0.2)', color: '#818cf8', border: '1px solid rgba(99,102,241,0.4)' },
    danger:  { bg: 'rgba(239,68,68,0.12)', color: 'var(--diff-rm-sign)', border: '1px solid rgba(239,68,68,0.2)' },
  }[variant];

  return (
    <Box
      as="button"
      py="4px"
      px={px ?? '12px'}
      borderRadius="4px"
      fontSize="11px"
      cursor={disabled ? 'not-allowed' : 'pointer'}
      fontFamily="inherit"
      disabled={disabled}
      opacity={disabled ? 0.5 : 1}
      onClick={onClick}
      {...styles}
    >
      {children}
    </Box>
  );
}
