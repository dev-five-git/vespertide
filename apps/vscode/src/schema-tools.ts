import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';

const fsp = fs.promises;

const ALLOWED_EXTENSIONS = new Set(['.json', '.yaml', '.yml']);
const MAX_DEPTH = 6;
const MAX_FILES = 300;
const MAX_LIST_BYTES = 20_000;
const MAX_READ_BYTES = 48 * 1024;

export type WorkspaceCtx = {
  rootReal: string;
  modelsDirReal: string;
  migrationsDirReal: string;
};

type SchemaFileEntry = { relPath: string; realPath: string };

function isContained(childReal: string, parentReal: string): boolean {
  const rel = path.relative(parentReal, childReal);
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel));
}

async function realpathOrNull(p: string): Promise<string | null> {
  try {
    return await fsp.realpath(p);
  } catch {
    return null;
  }
}

type VespertideJson = { modelsDir?: string; migrationsDir?: string };

/**
 * Picks the workspace folder the schema tools should operate on:
 * 1) the only folder, if there's exactly one
 * 2) the folder containing the active editor's document
 * 3) the only folder (among all open folders) with a vespertide.json at its root
 * Otherwise undefined — ambiguous, tools stay disabled.
 */
async function pickWorkspaceFolder(): Promise<vscode.WorkspaceFolder | undefined> {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) return undefined;
  if (folders.length === 1) return folders[0];

  const activeUri = vscode.window.activeTextEditor?.document.uri;
  if (activeUri) {
    const active = vscode.workspace.getWorkspaceFolder(activeUri);
    if (active) return active;
  }

  const withConfig: vscode.WorkspaceFolder[] = [];
  for (const folder of folders) {
    const exists = await realpathOrNull(path.join(folder.uri.fsPath, 'vespertide.json'));
    if (exists) withConfig.push(folder);
  }
  return withConfig.length === 1 ? withConfig[0] : undefined;
}

/**
 * Resolves the workspace + vespertide.json into a validated context, or undefined
 * if there's no (unambiguous) vespertide project to expose schema tools for.
 * modelsDir/migrationsDir are realpath-resolved and verified to stay inside the
 * workspace root — a config pointing outside (e.g. `../secret`, an absolute path,
 * or a symlinked directory that escapes the root) disables the tools entirely
 * rather than being trusted.
 */
export async function resolveWorkspaceContext(): Promise<WorkspaceCtx | undefined> {
  const folder = await pickWorkspaceFolder();
  if (!folder) return undefined;

  const rootReal = await realpathOrNull(folder.uri.fsPath);
  if (!rootReal) return undefined;

  let config: VespertideJson;
  try {
    const raw = await fsp.readFile(path.join(folder.uri.fsPath, 'vespertide.json'), 'utf-8');
    config = JSON.parse(raw) as VespertideJson;
  } catch {
    return undefined;
  }

  const modelsDirReal = await realpathOrNull(path.resolve(folder.uri.fsPath, config.modelsDir ?? 'models'));
  const migrationsDirReal = await realpathOrNull(
    path.resolve(folder.uri.fsPath, config.migrationsDir ?? 'migrations'),
  );

  if (!modelsDirReal || !isContained(modelsDirReal, rootReal)) return undefined;
  if (!migrationsDirReal || !isContained(migrationsDirReal, rootReal)) return undefined;

  return { rootReal, modelsDirReal, migrationsDirReal };
}

function toRelPath(rootReal: string, targetReal: string): string {
  return path.relative(rootReal, targetReal).split(path.sep).join('/');
}

async function walkDir(
  dirReal: string,
  rootReal: string,
  depth: number,
  entries: SchemaFileEntry[],
  warnings: string[],
): Promise<void> {
  if (entries.length >= MAX_FILES) return;
  if (depth > MAX_DEPTH) {
    warnings.push(`최대 탐색 깊이(${MAX_DEPTH})를 초과해 일부 디렉터리를 건너뛰었습니다.`);
    return;
  }

  let dirents: fs.Dirent[];
  try {
    dirents = await fsp.readdir(dirReal, { withFileTypes: true });
  } catch (err) {
    warnings.push(`${toRelPath(rootReal, dirReal)} 디렉터리를 읽을 수 없습니다: ${(err as Error).message}`);
    return;
  }

  for (const dirent of dirents) {
    if (entries.length >= MAX_FILES) {
      warnings.push(`파일 개수 제한(${MAX_FILES})에 도달해 나머지 항목을 생략했습니다.`);
      return;
    }

    const childPath = path.join(dirReal, dirent.name);

    // lstat (not stat) so symlinks are identified as such rather than followed.
    let lstat: fs.Stats;
    try {
      lstat = await fsp.lstat(childPath);
    } catch {
      continue;
    }
    if (lstat.isSymbolicLink()) continue;

    if (lstat.isDirectory()) {
      await walkDir(childPath, rootReal, depth + 1, entries, warnings);
      continue;
    }

    if (!lstat.isFile()) continue;
    if (!ALLOWED_EXTENSIONS.has(path.extname(dirent.name).toLowerCase())) continue;

    const childReal = await realpathOrNull(childPath);
    if (!childReal || !isContained(childReal, rootReal)) continue;

    entries.push({ relPath: toRelPath(rootReal, childReal), realPath: childReal });
  }
}

async function safeListFiles(
  ctx: WorkspaceCtx,
  dir: 'models' | 'migrations' | 'all',
): Promise<{ entries: SchemaFileEntry[]; warnings: string[] }> {
  const entries: SchemaFileEntry[] = [];
  const warnings: string[] = [];

  const roots: string[] = [];
  if (dir === 'models' || dir === 'all') roots.push(ctx.modelsDirReal);
  if (dir === 'migrations' || dir === 'all') roots.push(ctx.migrationsDirReal);

  for (const r of roots) {
    await walkDir(r, ctx.rootReal, 0, entries, warnings);
  }

  entries.sort((a, b) => a.relPath.localeCompare(b.relPath));
  return { entries, warnings };
}

function truncateUtf16Safe(s: string, maxLen: number): string {
  if (s.length <= maxLen) return s;
  let cut = maxLen;
  const code = s.charCodeAt(cut - 1);
  if (code >= 0xd800 && code <= 0xdbff) cut -= 1; // don't split a surrogate pair
  return s.slice(0, cut);
}

// ── Tool definitions (provider-neutral) ────────────────────────────────────────

export const SCHEMA_TOOLS = [
  {
    name: 'list_schema_files',
    description:
      'List the Vespertide model and migration files in this project. Call this first to discover ' +
      'what files exist before reading one — read_schema_file only accepts paths returned here.',
    parameters: {
      type: 'object',
      properties: {
        dir: {
          type: 'string',
          enum: ['models', 'migrations', 'all'],
          description: 'Which directory to list. Defaults to "all".',
        },
      },
    },
  },
  {
    name: 'read_schema_file',
    description:
      'Read the full contents of one model or migration file previously discovered via list_schema_files. ' +
      'The path must be exactly one of the relative paths returned by list_schema_files.',
    parameters: {
      type: 'object',
      properties: {
        path: {
          type: 'string',
          description: "Relative path as returned by list_schema_files, e.g. 'models/user.vespertide.json'.",
        },
      },
      required: ['path'],
    },
  },
] as const;

export type ToolCallResult = { ok: true; text: string } | { ok: false; error: string };

async function listSchemaFilesTool(ctx: WorkspaceCtx, args: unknown): Promise<ToolCallResult> {
  const dir = (args as { dir?: string })?.dir;
  const normalizedDir: 'models' | 'migrations' | 'all' =
    dir === 'models' || dir === 'migrations' ? dir : 'all';

  const { entries, warnings } = await safeListFiles(ctx, normalizedDir);

  let listing = entries.map((e) => e.relPath).join('\n');
  if (listing.length > MAX_LIST_BYTES) {
    const cutEntries = entries.filter((_, i) => entries.slice(0, i + 1).map((e) => e.relPath).join('\n').length <= MAX_LIST_BYTES);
    listing = `${cutEntries.map((e) => e.relPath).join('\n')}\n… (${entries.length - cutEntries.length}개 더 있음, 잘림)`;
  }

  const parts = [listing || '(파일 없음)'];
  if (warnings.length > 0) parts.push(`경고:\n${warnings.join('\n')}`);
  return { ok: true, text: parts.join('\n\n') };
}

async function readSchemaFileTool(ctx: WorkspaceCtx, args: unknown): Promise<ToolCallResult> {
  const requestedRaw = (args as { path?: string })?.path;
  if (!requestedRaw || typeof requestedRaw !== 'string') {
    return { ok: false, error: 'Missing required argument: path' };
  }
  const requested = requestedRaw.split(path.sep).join('/').replace(/^\/+/, '');

  // Re-derive the allowlist fresh on every read — read_schema_file may only return
  // files that a list_schema_files call would also return, right now.
  const { entries } = await safeListFiles(ctx, 'all');
  const match = entries.find((e) => e.relPath === requested);
  if (!match) {
    return {
      ok: false,
      error: `'${requested}' is not among the files returned by list_schema_files. Call list_schema_files again to see valid paths.`,
    };
  }

  let finalLstat: fs.Stats;
  try {
    finalLstat = await fsp.lstat(match.realPath);
  } catch (err) {
    return { ok: false, error: `Failed to stat file: ${(err as Error).message}` };
  }
  if (finalLstat.isSymbolicLink() || !finalLstat.isFile()) {
    return { ok: false, error: 'File is no longer a regular file (may have changed on disk).' };
  }

  let content: string;
  try {
    content = await fsp.readFile(match.realPath, 'utf-8');
  } catch (err) {
    return { ok: false, error: `Failed to read file: ${(err as Error).message}` };
  }

  const maxChars = MAX_READ_BYTES; // approximation: chars ~= bytes for this content, good enough as a cap
  if (content.length > maxChars) {
    content = `${truncateUtf16Safe(content, maxChars)}\n… (파일이 커서 잘림, 원본 ${content.length}자 중 ${maxChars}자만 표시)`;
  }

  return { ok: true, text: content };
}

/** Dispatches a tool call by name. Never throws — unknown tools/bad args become an error result. */
export async function runTool(ctx: WorkspaceCtx, name: string, args: unknown): Promise<ToolCallResult> {
  if (name === 'list_schema_files') return listSchemaFilesTool(ctx, args);
  if (name === 'read_schema_file') return readSchemaFileTool(ctx, args);
  return { ok: false, error: `Unknown tool: ${name}` };
}
