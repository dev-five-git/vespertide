import * as vscode from 'vscode';
import * as https from 'https';
import type { ConnectorService, ConnectorStatus, ChatMessage } from './messages';

const KEY_PREFIX = 'vespertide.connector.';

// ── SecretStorage helpers ──────────────────────────────────────────────────────

export async function saveConnector(
  service: ConnectorService,
  key: string,
  ctx: vscode.ExtensionContext,
): Promise<void> {
  await ctx.secrets.store(`${KEY_PREFIX}${service}`, key);
}

export async function deleteConnector(
  service: ConnectorService,
  ctx: vscode.ExtensionContext,
): Promise<void> {
  await ctx.secrets.delete(`${KEY_PREFIX}${service}`);
}

export async function loadConnectors(
  ctx: vscode.ExtensionContext,
): Promise<ConnectorStatus[]> {
  const services: ConnectorService[] = ['claude', 'openai', 'gemini', 'slack', 'notion', 'jira'];
  return Promise.all(
    services.map(async (service) => ({
      service,
      connected: !!(await ctx.secrets.get(`${KEY_PREFIX}${service}`)),
    })),
  );
}

async function getKey(service: ConnectorService, ctx: vscode.ExtensionContext): Promise<string> {
  const key = await ctx.secrets.get(`${KEY_PREFIX}${service}`);
  if (!key) throw new Error(`${service} 연결이 설정되지 않았습니다.`);
  return key;
}

// ── AI proxy ──────────────────────────────────────────────────────────────────

export async function callAI(
  service: ConnectorService,
  messages: ChatMessage[],
  context: string,
  ctx: vscode.ExtensionContext,
): Promise<string> {
  const key = await getKey(service, ctx);

  const system = `You are a database schema assistant for the Vespertide project. Answer in the same language the user uses. Current schema context:\n\n${context}`;

  if (service === 'claude') return callClaude(key, system, messages);
  if (service === 'openai') return callOpenAI(key, system, messages);
  if (service === 'gemini') return callGemini(key, system, messages);

  throw new Error(`${service}는 AI 서비스가 아닙니다. Slack/Notion/Jira 액션은 별도 명령을 사용하세요.`);
}

// ── Slack ─────────────────────────────────────────────────────────────────────

export async function sendSlack(
  text: string,
  ctx: vscode.ExtensionContext,
): Promise<void> {
  const webhookUrl = await getKey('slack', ctx);
  const body = JSON.stringify({ text });
  await postJson(webhookUrl, body, {});
}

// ── Notion ────────────────────────────────────────────────────────────────────

export async function createNotionPage(
  title: string,
  markdown: string,
  ctx: vscode.ExtensionContext,
): Promise<string> {
  const key = await getKey('notion', ctx);
  const config = vscode.workspace.getConfiguration('vespertide');
  const dbId: string = config.get('notionDatabaseId') ?? '';
  if (!dbId) throw new Error('vespertide.notionDatabaseId 설정이 필요합니다.');

  const body = JSON.stringify({
    parent: { database_id: dbId },
    properties: {
      Name: { title: [{ text: { content: title } }] },
    },
    children: [
      {
        object: 'block',
        type: 'paragraph',
        paragraph: {
          rich_text: [{ type: 'text', text: { content: markdown.slice(0, 2000) } }],
        },
      },
    ],
  });

  const resp = await postJson('https://api.notion.com/v1/pages', body, {
    Authorization: `Bearer ${key}`,
    'Notion-Version': '2022-06-28',
  });
  const parsed = JSON.parse(resp) as { url?: string };
  return parsed.url ?? '(Notion 페이지 생성 완료)';
}

// ── Jira ──────────────────────────────────────────────────────────────────────

export async function createJiraIssue(
  summary: string,
  description: string,
  ctx: vscode.ExtensionContext,
): Promise<string> {
  const key = await getKey('jira', ctx);
  const config = vscode.workspace.getConfiguration('vespertide');
  const baseUrl: string = config.get('jiraBaseUrl') ?? '';
  const projectKey: string = config.get('jiraProjectKey') ?? '';
  if (!baseUrl || !projectKey) {
    throw new Error('vespertide.jiraBaseUrl 및 vespertide.jiraProjectKey 설정이 필요합니다.');
  }

  const body = JSON.stringify({
    fields: {
      project: { key: projectKey },
      summary,
      description: { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text: description }] }] },
      issuetype: { name: 'Task' },
    },
  });

  const resp = await postJson(`${baseUrl}/rest/api/3/issue`, body, {
    Authorization: `Basic ${Buffer.from(key).toString('base64')}`,
  });
  const parsed = JSON.parse(resp) as { key?: string; self?: string };
  return parsed.key ? `${baseUrl}/browse/${parsed.key}` : '(Jira 이슈 생성 완료)';
}

// ── Claude API ────────────────────────────────────────────────────────────────

async function callClaude(apiKey: string, system: string, messages: ChatMessage[]): Promise<string> {
  const body = JSON.stringify({
    model: 'claude-opus-4-7',
    max_tokens: 4096,
    system,
    messages: messages.map((m) => ({ role: m.role, content: m.content })),
  });

  const resp = await postJson('https://api.anthropic.com/v1/messages', body, {
    'x-api-key': apiKey,
    'anthropic-version': '2023-06-01',
  });

  const parsed = JSON.parse(resp) as {
    content?: Array<{ type: string; text?: string }>;
    error?: { message: string };
  };
  if (parsed.error) throw new Error(parsed.error.message);
  return parsed.content?.find((c) => c.type === 'text')?.text ?? '';
}

// ── OpenAI API ────────────────────────────────────────────────────────────────

async function callOpenAI(apiKey: string, system: string, messages: ChatMessage[]): Promise<string> {
  const body = JSON.stringify({
    model: 'gpt-4o',
    messages: [
      { role: 'system', content: system },
      ...messages.map((m) => ({ role: m.role, content: m.content })),
    ],
  });

  const resp = await postJson('https://api.openai.com/v1/chat/completions', body, {
    Authorization: `Bearer ${apiKey}`,
  });

  const parsed = JSON.parse(resp) as {
    choices?: Array<{ message?: { content?: string } }>;
    error?: { message: string };
  };
  if (parsed.error) throw new Error(parsed.error.message);
  return parsed.choices?.[0]?.message?.content ?? '';
}

// ── Gemini API ────────────────────────────────────────────────────────────────

async function callGemini(apiKey: string, system: string, messages: ChatMessage[]): Promise<string> {
  const contents = [
    { role: 'user', parts: [{ text: system }] },
    { role: 'model', parts: [{ text: 'Understood. I will help with the schema.' }] },
    ...messages.map((m) => ({
      role: m.role === 'assistant' ? 'model' : 'user',
      parts: [{ text: m.content }],
    })),
  ];

  const body = JSON.stringify({ contents });
  const url = `https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key=${apiKey}`;

  const resp = await postJson(url, body, {});
  const parsed = JSON.parse(resp) as {
    candidates?: Array<{ content?: { parts?: Array<{ text?: string }> } }>;
    error?: { message: string };
  };
  if (parsed.error) throw new Error(parsed.error.message);
  return parsed.candidates?.[0]?.content?.parts?.[0]?.text ?? '';
}

// ── HTTP helper ───────────────────────────────────────────────────────────────

function postJson(
  urlOrPath: string,
  body: string,
  extraHeaders: Record<string, string>,
): Promise<string> {
  return new Promise((resolve, reject) => {
    let url: URL;
    try {
      url = new URL(urlOrPath);
    } catch {
      return reject(new Error(`잘못된 URL: ${urlOrPath}`));
    }

    const options: https.RequestOptions = {
      hostname: url.hostname,
      port: url.port || 443,
      path: url.pathname + url.search,
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body),
        ...extraHeaders,
      },
    };

    const req = https.request(options, (res) => {
      const chunks: Buffer[] = [];
      res.on('data', (c: Buffer) => chunks.push(c));
      res.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf-8');
        const code = res.statusCode ?? 0;
        if (code >= 200 && code < 300) {
          resolve(text);
        } else {
          reject(new Error(`HTTP ${code}: ${text.slice(0, 300)}`));
        }
      });
    });

    req.on('error', reject);
    req.write(body);
    req.end();
  });
}
