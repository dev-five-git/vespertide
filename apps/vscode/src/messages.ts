export type OrmType = 'prisma' | 'typeorm' | 'drizzle' | 'jpa' | 'sqlalchemy' | 'gorm';
export type DbDialect = 'postgres' | 'mysql' | 'sqlite';

/** Opaque schema object — passed between host and webview */
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
