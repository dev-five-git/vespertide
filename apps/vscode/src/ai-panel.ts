/// <reference types="node" />
import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import type { WebviewMessage, HostMessage } from './messages';
import {
  callAI,
  loadConnectors,
  saveConnector,
  deleteConnector,
  sendSlack,
  createNotionPage,
  createJiraIssue,
  checkOllama,
} from './connectors';

export class AIAgentPanel {
  public static readonly viewType = 'vespertide.aiAgent';
  private static _instance: AIAgentPanel | undefined;

  private readonly _panel: vscode.WebviewPanel;
  private _disposables: vscode.Disposable[] = [];
  private _pendingAbort: AbortController | undefined;

  public static createOrShow(extensionUri: vscode.Uri, ctx: vscode.ExtensionContext): void {
    if (AIAgentPanel._instance) {
      AIAgentPanel._instance._panel.reveal(vscode.ViewColumn.Two);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      AIAgentPanel.viewType,
      'Vespertide AI Agent',
      vscode.ViewColumn.Two,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [
          vscode.Uri.joinPath(extensionUri, 'dist', 'webview'),
          vscode.Uri.joinPath(extensionUri, 'assets'),
        ],
      },
    );

    AIAgentPanel._instance = new AIAgentPanel(panel, extensionUri, ctx);
  }

  private constructor(
    panel: vscode.WebviewPanel,
    private readonly _extensionUri: vscode.Uri,
    private readonly _ctx: vscode.ExtensionContext,
  ) {
    this._panel = panel;
    this._panel.webview.html = this._buildHtml();

    this._panel.onDidDispose(() => this._dispose(), null, this._disposables);
    this._panel.webview.onDidReceiveMessage(
      async (msg: WebviewMessage) => this._handleMessage(msg),
      null,
      this._disposables,
    );

    setTimeout(() => {
      loadConnectors(this._ctx).then((connectors) => {
        this._post({ type: 'connector_status', connectors });
      }).catch(console.error);
    }, 300);
  }

  private async _handleMessage(msg: WebviewMessage): Promise<void> {
    try {
      switch (msg.type) {
        case 'connector_save': {
          await saveConnector(msg.service, msg.key, this._ctx);
          const connectors = await loadConnectors(this._ctx);
          this._post({ type: 'connector_status', connectors });
          break;
        }
        case 'connector_delete': {
          await deleteConnector(msg.service, this._ctx);
          const connectors = await loadConnectors(this._ctx);
          this._post({ type: 'connector_status', connectors });
          break;
        }
        case 'connector_load': {
          const connectors = await loadConnectors(this._ctx);
          this._post({ type: 'connector_status', connectors });
          break;
        }
        case 'open_external': {
          await vscode.env.openExternal(vscode.Uri.parse(msg.url));
          break;
        }
        case 'ollama_check': {
          const status = await checkOllama();
          this._post({ type: 'ollama_status', ...status });
          break;
        }
        case 'ai_chat': {
          const controller = new AbortController();
          this._pendingAbort = controller;
          try {
            if (msg.service === 'slack') {
              const last = msg.messages[msg.messages.length - 1];
              await sendSlack(last?.content ?? '', this._ctx, controller.signal);
              this._post({ type: 'ai_response', content: 'Slack 메시지를 전송했습니다.', done: true });
            } else if (msg.service === 'notion') {
              const last = msg.messages[msg.messages.length - 1];
              const url = await createNotionPage('Vespertide 스키마', last?.content ?? msg.context, this._ctx, controller.signal);
              this._post({ type: 'ai_response', content: `Notion 페이지가 생성되었습니다:\n${url}`, done: true });
            } else if (msg.service === 'jira') {
              const last = msg.messages[msg.messages.length - 1];
              const url = await createJiraIssue('Vespertide 작업', last?.content ?? '', this._ctx, controller.signal);
              this._post({ type: 'ai_response', content: `Jira 이슈가 생성되었습니다:\n${url}`, done: true });
            } else {
              const text = await callAI(msg.service, msg.messages, msg.context, this._ctx, controller.signal);
              this._post({ type: 'ai_response', content: text, done: true });
            }
          } finally {
            if (this._pendingAbort === controller) this._pendingAbort = undefined;
          }
          break;
        }
        case 'ai_chat_cancel': {
          this._pendingAbort?.abort();
          break;
        }
      }
    } catch (err) {
      if (err instanceof Error && err.name === 'AbortError') {
        this._post({ type: 'ai_cancelled' });
        return;
      }
      const message = err instanceof Error ? err.message : String(err);
      this._post({ type: 'error', message });
    }
  }

  private _post(msg: HostMessage): void {
    this._panel.webview.postMessage(msg);
  }

  private _buildHtml(): string {
    const webview = this._panel.webview;
    const webviewDist = vscode.Uri.joinPath(this._extensionUri, 'dist', 'webview');
    const htmlPath = path.join(this._extensionUri.fsPath, 'dist', 'webview', 'ai-agent.html');

    if (fs.existsSync(htmlPath)) {
      const nonce = randomNonce();
      let html = fs.readFileSync(htmlPath, 'utf-8');

      html = html.replace(/(src|href)="(\.\/[^"]+|\/[^"]+)"/g, (_m: string, attr: string, val: string) => {
        const relative = val.replace(/^\//, '').replace(/^\.\//, '');
        const uri = webview.asWebviewUri(vscode.Uri.joinPath(webviewDist, relative));
        return `${attr}="${uri}"`;
      });

      const csp = [
        `default-src 'none'`,
        `script-src 'nonce-${nonce}' 'unsafe-eval' ${webview.cspSource}`,
        `style-src ${webview.cspSource} 'unsafe-inline'`,
        `img-src ${webview.cspSource} data:`,
        `font-src ${webview.cspSource}`,
      ].join('; ');

      html = html.replace('<head>', `<head><meta http-equiv="Content-Security-Policy" content="${csp}">`);
      html = html.replace(/<script(?!.*nonce)/g, `<script nonce="${nonce}"`);
      return html;
    }

    return `<!DOCTYPE html><html><body style="font-family:sans-serif;color:#ccc;padding:24px">
      <p>AI Agent 패널 — 빌드를 먼저 실행하세요.</p>
      <code>cd apps/vscode/webview && npm run build</code>
    </body></html>`;
  }

  private _dispose(): void {
    AIAgentPanel._instance = undefined;
    this._panel.dispose();
    for (const d of this._disposables) d.dispose();
    this._disposables = [];
  }
}

function randomNonce(): string {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  return Array.from({ length: 32 }, () => chars[Math.floor(Math.random() * chars.length)]).join('');
}
