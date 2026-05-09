import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import type { WebviewMessage, HostMessage } from './messages';
import { initWasm, parseOrm, renderErd, convertOrm, generateMigration } from './wasm-host';
import {
  exportSvg,
  exportPdf,
  exportMcp,
  setCurrentSchema,
  setCurrentSvg,
} from './export';

export class VespertideWebviewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = 'vespertide.mainPanel';

  private _view?: vscode.WebviewView;

  constructor(private readonly _extensionUri: vscode.Uri) {}

  resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
  ): void {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [
        vscode.Uri.joinPath(this._extensionUri, 'dist', 'webview'),
        vscode.Uri.joinPath(this._extensionUri, 'assets'),
      ],
    };

    webviewView.webview.html = this._buildHtml(webviewView.webview);

    webviewView.webview.onDidReceiveMessage(async (msg: WebviewMessage) => {
      await this._handleMessage(msg);
    });

    // Initialize WASM after panel resolves
    initWasm(this._extensionUri.fsPath).catch(console.error);
  }

  // ── Message handler ────────────────────────────────────────────────────────

  private async _handleMessage(msg: WebviewMessage): Promise<void> {
    try {
      switch (msg.type) {
        case 'parse_orm': {
          const schema = await parseOrm(msg.source, msg.orm);
          setCurrentSchema(schema);
          const svg = await renderErd(schema);
          setCurrentSvg(svg);
          this._post({ type: 'erd_updated', svg });
          break;
        }

        case 'convert_orm': {
          const source = await convertOrm(msg.source, msg.from, msg.to);
          this._post({ type: 'orm_converted', source });
          break;
        }

        case 'generate_migration': {
          const [postgres, mysql, sqlite] = await Promise.all([
            generateMigration(msg.schema, 'postgres'),
            generateMigration(msg.schema, 'mysql'),
            generateMigration(msg.schema, 'sqlite'),
          ]);
          this._post({ type: 'migration_updated', postgres, mysql, sqlite });
          break;
        }

        case 'export_svg': {
          const filePath = await exportSvg();
          this._post({ type: 'export_done', path: filePath });
          break;
        }

        case 'export_pdf': {
          const filePath = await exportPdf();
          this._post({ type: 'export_done', path: filePath });
          break;
        }

        case 'export_mcp': {
          await exportMcp(msg.schema);
          this._post({ type: 'export_done' });
          break;
        }
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      this._post({ type: 'error', message });
    }
  }

  private _post(msg: HostMessage): void {
    this._view?.webview.postMessage(msg);
  }

  // ── HTML construction ──────────────────────────────────────────────────────

  private _buildHtml(webview: vscode.Webview): string {
    const webviewDist = vscode.Uri.joinPath(this._extensionUri, 'dist', 'webview');
    const indexPath = path.join(this._extensionUri.fsPath, 'dist', 'webview', 'index.html');

    if (fs.existsSync(indexPath)) {
      const nonce = randomNonce();
      let html = fs.readFileSync(indexPath, 'utf-8');

      // Rewrite asset paths to webview URIs
      html = html.replace(/(src|href)="(\.\/[^"]+|\/[^"]+)"/g, (_m, attr, val) => {
        const relative = val.replace(/^\//, '').replace(/^\.\//, '');
        const uri = webview.asWebviewUri(vscode.Uri.joinPath(webviewDist, relative));
        return `${attr}="${uri}"`;
      });

      // Inject CSP
      const csp = [
        `default-src 'none'`,
        `script-src 'nonce-${nonce}' 'unsafe-eval'`,
        `style-src ${webview.cspSource} 'unsafe-inline'`,
        `img-src ${webview.cspSource} data:`,
        `font-src ${webview.cspSource}`,
      ].join('; ');

      html = html.replace('<head>', `<head><meta http-equiv="Content-Security-Policy" content="${csp}">`);
      // Add nonce to all inline scripts
      html = html.replace(/<script(?!.*nonce)/g, `<script nonce="${nonce}"`);
      return html;
    }

    // Dev / first-run fallback
    return buildFallbackHtml();
  }
}

function randomNonce(): string {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  return Array.from({ length: 32 }, () => chars[Math.floor(Math.random() * chars.length)]).join('');
}

function buildFallbackHtml(): string {
  return /* html */ `<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="UTF-8"/>
  <meta name="viewport" content="width=device-width,initial-scale=1"/>
  <title>Vespertide</title>
  <style>
    body {
      margin: 0; padding: 24px;
      font-family: var(--vscode-font-family, sans-serif);
      color: var(--vscode-foreground, #ccc);
      background: var(--vscode-sideBar-background, #252526);
      font-size: 13px;
    }
    h2 { margin: 0 0 12px; font-size: 15px; }
    p  { margin: 6px 0; opacity: 0.7; line-height: 1.6; }
    code {
      display: inline-block; margin: 2px 0;
      background: var(--vscode-textCodeBlock-background, rgba(255,255,255,0.08));
      padding: 2px 8px; border-radius: 3px; font-size: 12px;
    }
  </style>
</head>
<body>
  <h2>Vespertide</h2>
  <p>Webview 빌드가 필요합니다.</p>
  <p>1. <code>cd apps/vscode/webview &amp;&amp; npm install &amp;&amp; npm run build</code></p>
  <p>2. <code>cd apps/vscode &amp;&amp; npm install &amp;&amp; npm run build:ext</code></p>
  <p>3. VS Code에서 <code>F5</code> 로 Extension 재시작</p>
</body>
</html>`;
}
