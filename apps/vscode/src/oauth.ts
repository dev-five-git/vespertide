/// <reference types="node" />
import * as http from 'http';
import * as crypto from 'crypto';
import * as https from 'https';
import * as vscode from 'vscode';

// ── OAuth 설정 ────────────────────────────────────────────────────────────────

interface OAuthDef {
  authUrl: string;
  tokenUrl: string;
  scopes: string[];
  pkce: boolean;
  extraAuthParams?: Record<string, string>;
  extractToken: (body: Record<string, unknown>) => string;
}

const OAUTH_DEFS: Record<string, OAuthDef> = {
  slack: {
    authUrl: 'https://slack.com/oauth/v2/authorize',
    tokenUrl: 'https://slack.com/api/oauth.v2.access',
    scopes: ['incoming-webhook', 'chat:write'],
    pkce: false,
    // Slack OAuth returns the webhook URL in the response body
    extractToken: (b) => {
      const hook = (b as { incoming_webhook?: { url?: string } }).incoming_webhook?.url;
      if (hook) return hook;
      const token = b['access_token'];
      if (typeof token === 'string') return token;
      throw new Error('Slack OAuth 응답에서 webhook URL을 찾을 수 없습니다');
    },
  },
  notion: {
    authUrl: 'https://api.notion.com/v1/oauth/authorize',
    tokenUrl: 'https://api.notion.com/v1/oauth/token',
    scopes: [],
    pkce: false,
    extraAuthParams: { owner: 'user' },
    // Notion access_token is used as Bearer — same as existing API key flow
    extractToken: (b) => {
      if (typeof b['access_token'] === 'string') return b['access_token'];
      throw new Error('Notion OAuth 응답에서 access_token을 찾을 수 없습니다');
    },
  },
  jira: {
    authUrl: 'https://auth.atlassian.com/authorize',
    tokenUrl: 'https://auth.atlassian.com/oauth/token',
    scopes: ['read:jira-work', 'write:jira-work', 'offline_access'],
    pkce: true,
    extraAuthParams: { audience: 'api.atlassian.com', prompt: 'consent' },
    // Store with __bearer__ prefix so createJiraIssue knows to use Bearer auth
    extractToken: (b) => {
      if (typeof b['access_token'] === 'string') return `__bearer__:${b['access_token']}`;
      throw new Error('Jira OAuth 응답에서 access_token을 찾을 수 없습니다');
    },
  },
  gemini: {
    authUrl: 'https://accounts.google.com/o/oauth2/v2/auth',
    tokenUrl: 'https://oauth2.googleapis.com/token',
    scopes: ['https://www.googleapis.com/auth/generative-language'],
    pkce: true,
    extraAuthParams: { access_type: 'offline', prompt: 'consent' },
    // Store with __bearer__ prefix so callGemini uses Authorization header
    extractToken: (b) => {
      if (typeof b['access_token'] === 'string') return `__bearer__:${b['access_token']}`;
      throw new Error('Google OAuth 응답에서 access_token을 찾을 수 없습니다');
    },
  },
};

export const OAUTH_SERVICES = new Set(Object.keys(OAUTH_DEFS));

const TOKEN_EXCHANGE_TIMEOUT_MS = 30_000;

// ── PKCE helpers ──────────────────────────────────────────────────────────────

function randomHex(bytes: number) { return crypto.randomBytes(bytes).toString('hex'); }
function b64url(buf: Buffer) { return buf.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, ''); }

function makePKCE(): { verifier: string; challenge: string } {
  const verifier = b64url(crypto.randomBytes(32));
  const challenge = b64url(crypto.createHash('sha256').update(verifier).digest());
  return { verifier, challenge };
}

// ── Main OAuth flow ───────────────────────────────────────────────────────────

export async function startOAuthFlow(
  service: string,
  _ctx: vscode.ExtensionContext,
): Promise<string> {
  const def = OAUTH_DEFS[service];
  if (!def) throw new Error(`${service}는 OAuth를 지원하지 않습니다`);

  const cfg = vscode.workspace.getConfiguration('vespertide');
  const clientId = cfg.get<string>(`oauth.${service}.clientId`);
  const clientSecret = cfg.get<string>(`oauth.${service}.clientSecret`);

  if (!clientId) {
    throw new Error(
      `${service} OAuth Client ID가 설정되지 않았습니다.\n` +
      `VS Code 설정에서 vespertide.oauth.${service}.clientId를 입력하세요.`
    );
  }
  if (!def.pkce && !clientSecret) {
    throw new Error(
      `${service} OAuth Client Secret이 설정되지 않았습니다.\n` +
      `VS Code 설정에서 vespertide.oauth.${service}.clientSecret을 입력하세요.`
    );
  }

  const pkce = def.pkce ? makePKCE() : null;
  const state = randomHex(16);

  return new Promise<string>((resolve, reject) => {
    const server = http.createServer((req, res) => {
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });

      const urlObj = new URL(req.url ?? '/', 'http://localhost');
      if (urlObj.pathname !== '/callback') { res.end(); return; }

      const code = urlObj.searchParams.get('code');
      const returnedState = urlObj.searchParams.get('state');
      const oauthError = urlObj.searchParams.get('error');

      const html = oauthError
        ? `<body style="font-family:sans-serif;text-align:center;padding:60px"><h2>❌ 인증 실패</h2><p>${oauthError}</p></body>`
        : `<body style="font-family:sans-serif;text-align:center;padding:60px"><h2>✅ 인증 완료!</h2><p>VS Code로 돌아가세요.</p><script>setTimeout(()=>window.close(),2000)</script></body>`;
      res.end(`<!DOCTYPE html><html><meta charset="UTF-8">${html}</html>`);
      server.close();

      if (oauthError) { reject(new Error(`OAuth 오류: ${oauthError}`)); return; }
      if (returnedState !== state) { reject(new Error('State 불일치 — CSRF 가능성')); return; }
      if (!code) { reject(new Error('인증 코드가 없습니다')); return; }

      exchangeToken(def, code, clientId, clientSecret ?? '', redirectUri, pkce?.verifier)
        .then((body) => {
          const token = def.extractToken(body);
          resolve(token);
        })
        .catch(reject);
    });

    let redirectUri = '';
    server.listen(0, '127.0.0.1', () => {
      const port = (server.address() as { port: number }).port;
      redirectUri = `http://localhost:${port}/callback`;

      const authUrl = new URL(def.authUrl);
      authUrl.searchParams.set('client_id', clientId);
      authUrl.searchParams.set('redirect_uri', redirectUri);
      authUrl.searchParams.set('response_type', 'code');
      authUrl.searchParams.set('state', state);
      if (def.scopes.length) authUrl.searchParams.set('scope', def.scopes.join(' '));
      if (pkce) {
        authUrl.searchParams.set('code_challenge', pkce.challenge);
        authUrl.searchParams.set('code_challenge_method', 'S256');
      }
      for (const [k, v] of Object.entries(def.extraAuthParams ?? {})) {
        authUrl.searchParams.set(k, v);
      }

      vscode.env.openExternal(vscode.Uri.parse(authUrl.toString()));
    });

    // 5분 타임아웃
    setTimeout(() => {
      server.close();
      reject(new Error('인증 시간 초과 (5분)'));
    }, 5 * 60 * 1000);
  });
}

// ── Token exchange ────────────────────────────────────────────────────────────

async function exchangeToken(
  def: OAuthDef,
  code: string,
  clientId: string,
  clientSecret: string,
  redirectUri: string,
  codeVerifier?: string,
): Promise<Record<string, unknown>> {
  const params = new URLSearchParams({
    grant_type: 'authorization_code',
    code,
    redirect_uri: redirectUri,
    client_id: clientId,
  });
  if (clientSecret && !def.pkce) params.set('client_secret', clientSecret);
  if (codeVerifier) params.set('code_verifier', codeVerifier);

  const bodyStr = params.toString();
  const url = new URL(def.tokenUrl);

  const headers: Record<string, string | number> = {
    'Content-Type': 'application/x-www-form-urlencoded',
    'Content-Length': Buffer.byteLength(bodyStr),
    'Accept': 'application/json',
  };
  // Notion uses HTTP Basic auth for client credentials
  if (def.tokenUrl.includes('notion') && clientSecret) {
    headers['Authorization'] = 'Basic ' + Buffer.from(`${clientId}:${clientSecret}`).toString('base64');
  }

  const text = await new Promise<string>((resolve, reject) => {
    const req = https.request(
      {
        hostname: url.hostname,
        port: 443,
        path: url.pathname + url.search,
        method: 'POST',
        timeout: TOKEN_EXCHANGE_TIMEOUT_MS,
        headers,
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (c: Buffer) => chunks.push(c));
        res.on('end', () => {
          const t = Buffer.concat(chunks).toString('utf-8');
          if ((res.statusCode ?? 0) >= 400) {
            reject(new Error(`Token exchange HTTP ${res.statusCode}: ${t.slice(0, 200)}`));
          } else {
            resolve(t);
          }
        });
      },
    );
    req.on('error', reject);
    req.on('timeout', () => {
      req.destroy();
      reject(new Error(`Token exchange 요청 시간 초과 (${TOKEN_EXCHANGE_TIMEOUT_MS / 1000}초)`));
    });
    req.write(bodyStr);
    req.end();
  });

  return JSON.parse(text) as Record<string, unknown>;
}
