/**
 * VS Code Webview API shim.
 * acquireVsCodeApi() is injected by VS Code at runtime.
 * In browser dev mode this gracefully no-ops.
 */

declare function acquireVsCodeApi(): {
  postMessage(message: unknown): void;
  getState(): unknown;
  setState(state: unknown): void;
};

export type OrmType = 'prisma' | 'typeorm' | 'drizzle' | 'jpa' | 'sqlalchemy' | 'gorm';
export type DbDialect = 'postgres' | 'mysql' | 'sqlite';
export type Schema = Record<string, unknown>;

// Webview → Host
export type WebviewMessage =
  | { type: 'parse_orm'; source: string; orm: OrmType }
  | { type: 'convert_orm'; source: string; from: OrmType; to: OrmType }
  | { type: 'generate_migration'; schema: Schema; db: DbDialect }
  | { type: 'export_svg' }
  | { type: 'export_pdf' }
  | { type: 'export_mcp'; schema: Schema };

// Host → Webview
export type HostMessage =
  | { type: 'erd_updated'; svg: string }
  | { type: 'orm_converted'; source: string }
  | { type: 'migration_updated'; postgres: string; mysql: string; sqlite: string }
  | { type: 'export_done'; path?: string }
  | { type: 'error'; message: string };

// Singleton API handle
let _api: ReturnType<typeof acquireVsCodeApi> | undefined;
function getApi() {
  if (!_api && typeof acquireVsCodeApi === 'function') {
    _api = acquireVsCodeApi();
  }
  return _api;
}

export function postMessage(msg: WebviewMessage): void {
  getApi()?.postMessage(msg);
}

export function onMessage(handler: (msg: HostMessage) => void): () => void {
  const listener = (event: MessageEvent) => handler(event.data as HostMessage);
  window.addEventListener('message', listener);
  return () => window.removeEventListener('message', listener);
}
