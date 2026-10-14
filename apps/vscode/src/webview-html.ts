import * as vscode from 'vscode';

export function randomNonce(): string {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
  return Array.from({ length: 32 }, () => chars[Math.floor(Math.random() * chars.length)]).join('');
}

/** Rewrites asset `src`/`href` attrs to webview URIs and injects a CSP + script nonce. */
export function injectWebviewSecurity(
  html: string,
  webview: vscode.Webview,
  webviewDist: vscode.Uri,
): string {
  const nonce = randomNonce();

  let out = html.replace(/(src|href)="(\.\/[^"]+|\/[^"]+)"/g, (_m, attr: string, val: string) => {
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

  out = out.replace('<head>', `<head><meta http-equiv="Content-Security-Policy" content="${csp}">`);
  out = out.replace(/<script(?!.*nonce)/g, `<script nonce="${nonce}"`);
  return out;
}
