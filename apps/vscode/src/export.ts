import * as vscode from 'vscode';
import type { DbDialect, OrmType, Schema } from './messages';
import { svgToPdf } from './wasm-host';

let currentSchema: Schema = {};
let currentSvg = '';

function defaultSaveUri(filename: string): vscode.Uri {
  const folder = vscode.workspace.workspaceFolders?.[0]?.uri;
  return folder ? vscode.Uri.joinPath(folder, filename) : vscode.Uri.file(filename);
}

export function setCurrentSchema(schema: Schema): void {
  currentSchema = schema;
}

export function setCurrentSvg(svg: string): void {
  currentSvg = svg;
}

// ── SQL ────────────────────────────────────────────────────────────────────────

export async function exportSql(content: string, dialect: DbDialect): Promise<string | undefined> {
  const suffix = dialect === 'postgres' ? 'postgres' : dialect === 'mysql' ? 'mysql' : 'sqlite';
  const uri = await vscode.window.showSaveDialog({
    defaultUri: defaultSaveUri(`migration.${suffix}.sql`),
    filters: { 'SQL 파일': ['sql'] },
  });
  if (!uri) return undefined;
  await vscode.workspace.fs.writeFile(uri, Buffer.from(content, 'utf-8'));
  vscode.window.showInformationMessage(`Vespertide: SQL 저장 완료 — ${uri.fsPath}`);
  return uri.fsPath;
}

// ── Schema / ORM source ────────────────────────────────────────────────────────

export async function exportSchema(content: string, ormType: OrmType): Promise<string | undefined> {
  const extMap: Record<OrmType, string> = {
    prisma:     'prisma',
    drizzle:    'ts',
    typeorm:    'ts',
    gorm:       'go',
    jpa:        'java',
    sqlalchemy: 'py',
  };
  const ext = extMap[ormType] ?? 'txt';
  const uri = await vscode.window.showSaveDialog({
    defaultUri: defaultSaveUri(`schema.${ext}`),
    filters: {},
  });
  if (!uri) return undefined;
  await vscode.workspace.fs.writeFile(uri, Buffer.from(content, 'utf-8'));
  vscode.window.showInformationMessage(`Vespertide: 스키마 저장 완료 — ${uri.fsPath}`);
  return uri.fsPath;
}

// ── SVG ────────────────────────────────────────────────────────────────────────

export async function exportSvg(): Promise<string | undefined> {
  if (!currentSvg) {
    vscode.window.showErrorMessage('Vespertide: ERD SVG가 아직 생성되지 않았습니다.');
    return undefined;
  }

  const uri = await vscode.window.showSaveDialog({
    defaultUri: defaultSaveUri('erd-diagram.svg'),
    filters: { 'SVG 파일': ['svg'] },
  });
  if (!uri) return undefined;

  await vscode.workspace.fs.writeFile(uri, Buffer.from(currentSvg, 'utf-8'));
  vscode.window.showInformationMessage(`Vespertide: SVG 저장 완료 — ${uri.fsPath}`);
  return uri.fsPath;
}

// ── PDF ────────────────────────────────────────────────────────────────────────

export async function exportPdf(): Promise<string | undefined> {
  if (!currentSvg) {
    vscode.window.showErrorMessage('Vespertide: ERD SVG가 아직 생성되지 않았습니다.');
    return undefined;
  }

  const uri = await vscode.window.showSaveDialog({
    defaultUri: defaultSaveUri('erd-diagram.pdf'),
    filters: { 'PDF 파일': ['pdf'] },
  });
  if (!uri) return undefined;

  let pdfBuffer = await svgToPdf(currentSvg);

  if (!pdfBuffer) {
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports, @typescript-eslint/no-explicit-any
      const { jsPDF } = require('jspdf') as any;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const doc = new jsPDF({ orientation: 'landscape', unit: 'px', format: [800, 600] }) as any;
      const svgDataUrl = 'data:image/svg+xml;base64,' + Buffer.from(currentSvg).toString('base64');
      doc.addImage(svgDataUrl, 'JPEG', 0, 0, 800, 600);
      pdfBuffer = Buffer.from(doc.output('arraybuffer') as ArrayBuffer);
    } catch {
      vscode.window.showErrorMessage(
        'Vespertide: PDF 변환 실패 — jspdf가 설치되어 있지 않습니다. (npm install jspdf)',
      );
      return undefined;
    }
  }

  await vscode.workspace.fs.writeFile(uri, pdfBuffer);
  vscode.window.showInformationMessage(`Vespertide: PDF 저장 완료 — ${uri.fsPath}`);
  return uri.fsPath;
}

// ── JSON schema ────────────────────────────────────────────────────────────────

export async function exportSchemaJson(): Promise<string | undefined> {
  if (!Object.keys(currentSchema).length) {
    vscode.window.showErrorMessage('Vespertide: 스키마가 아직 생성되지 않았습니다.');
    return undefined;
  }
  const uri = await vscode.window.showSaveDialog({
    defaultUri: defaultSaveUri('schema.json'),
    filters: { 'JSON 파일': ['json'] },
  });
  if (!uri) return undefined;
  await vscode.workspace.fs.writeFile(uri, Buffer.from(JSON.stringify(currentSchema, null, 2), 'utf-8'));
  vscode.window.showInformationMessage(`Vespertide: JSON 저장 완료 — ${uri.fsPath}`);
  return uri.fsPath;
}
