import * as vscode from 'vscode';
import * as http from 'http';
import * as https from 'https';
import type { Schema } from './messages';
import { svgToPdf } from './wasm-host';

let currentSchema: Schema = {};
let currentSvg = '';

export function setCurrentSchema(schema: Schema): void {
  currentSchema = schema;
}

export function setCurrentSvg(svg: string): void {
  currentSvg = svg;
}

export async function exportSvg(): Promise<string | undefined> {
  if (!currentSvg) {
    vscode.window.showErrorMessage('Vespertide: ERD SVG가 아직 생성되지 않았습니다.');
    return undefined;
  }

  const uri = await vscode.window.showSaveDialog({
    defaultUri: vscode.Uri.file('erd.svg'),
    filters: { 'SVG 파일': ['svg'] },
  });
  if (!uri) return undefined;

  await vscode.workspace.fs.writeFile(uri, Buffer.from(currentSvg, 'utf-8'));
  vscode.window.showInformationMessage(`Vespertide: SVG 저장 완료 — ${uri.fsPath}`);
  return uri.fsPath;
}

export async function exportPdf(): Promise<string | undefined> {
  if (!currentSvg) {
    vscode.window.showErrorMessage('Vespertide: ERD SVG가 아직 생성되지 않았습니다.');
    return undefined;
  }

  const uri = await vscode.window.showSaveDialog({
    defaultUri: vscode.Uri.file('erd.pdf'),
    filters: { 'PDF 파일': ['pdf'] },
  });
  if (!uri) return undefined;

  // 1) Try WASM svg_to_pdf
  let pdfBuffer = await svgToPdf(currentSvg);

  // 2) Fallback: jsPDF
  if (!pdfBuffer) {
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports, @typescript-eslint/no-explicit-any
      const { jsPDF } = require('jspdf') as any;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const doc = new jsPDF({ orientation: 'landscape', unit: 'px', format: [800, 600] }) as any;
      const svgDataUrl =
        'data:image/svg+xml;base64,' + Buffer.from(currentSvg).toString('base64');
      doc.addImage(svgDataUrl, 'JPEG', 0, 0, 800, 600);
      pdfBuffer = Buffer.from(doc.output('arraybuffer') as ArrayBuffer);
    } catch {
      vscode.window.showErrorMessage(
        'Vespertide: PDF 변환 실패 — jspdf가 설치되어 있지 않습니다. (npm install jspdf)'
      );
      return undefined;
    }
  }

  await vscode.workspace.fs.writeFile(uri, pdfBuffer);
  vscode.window.showInformationMessage(`Vespertide: PDF 저장 완료 — ${uri.fsPath}`);
  return uri.fsPath;
}

export async function exportMcp(schema: Schema): Promise<void> {
  const config = vscode.workspace.getConfiguration('vespertide');
  const endpoint: string = config.get('mcpEndpoint') ?? 'http://localhost:3456';

  const body = JSON.stringify(schema);

  let url: URL;
  try {
    url = new URL(endpoint);
  } catch {
    vscode.window.showErrorMessage(`Vespertide: 잘못된 MCP 엔드포인트 URL — ${endpoint}`);
    return;
  }

  const options: http.RequestOptions = {
    hostname: url.hostname,
    port: url.port || (url.protocol === 'https:' ? 443 : 80),
    path: url.pathname || '/',
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Content-Length': Buffer.byteLength(body),
    },
  };

  await new Promise<void>((resolve, reject) => {
    const transport = url.protocol === 'https:' ? https : http;
    const req = transport.request(options, (res) => {
      const { statusCode = 0 } = res;
      if (statusCode >= 200 && statusCode < 300) {
        resolve();
      } else {
        reject(new Error(`HTTP ${statusCode}`));
      }
    });
    req.on('error', reject);
    req.write(body);
    req.end();
  }).then(
    () => vscode.window.showInformationMessage(`Vespertide: MCP 전송 완료 → ${endpoint}`),
    (err: Error) =>
      vscode.window.showErrorMessage(`Vespertide: MCP 전송 실패 — ${err.message}`)
  );
}
