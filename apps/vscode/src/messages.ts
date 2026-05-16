export type OrmType = 'prisma' | 'typeorm' | 'drizzle' | 'jpa' | 'sqlalchemy' | 'gorm';
export type DbDialect = 'postgres' | 'mysql' | 'sqlite';
export type ConnectorService = 'claude' | 'openai' | 'gemini' | 'slack' | 'notion' | 'jira';
export type ConnectorStatus = { service: ConnectorService; connected: boolean };
export type ChatMessage = { role: 'user' | 'assistant'; content: string };

/** Opaque schema object — passed between host and webview */
export type Schema = Record<string, unknown>;

// Webview → Host
export type WebviewMessage =
  | { type: 'parse_orm'; source: string; orm: OrmType }
  | { type: 'convert_orm'; source: string; from: OrmType; to: OrmType }
  | { type: 'generate_migration'; schema: Schema; db: DbDialect }
  | { type: 'export_svg' }
  | { type: 'export_pdf' }
  | { type: 'export_sql'; content: string; dialect: DbDialect }
  | { type: 'export_schema'; content: string; ormType: OrmType }
  | { type: 'connector_save'; service: ConnectorService; key: string }
  | { type: 'connector_delete'; service: ConnectorService }
  | { type: 'connector_load' }
  | { type: 'ai_chat'; service: ConnectorService; messages: ChatMessage[]; context: string };

// Host → Webview
export type HostMessage =
  | { type: 'erd_updated'; svg: string }
  | { type: 'orm_converted'; source: string }
  | { type: 'migration_updated'; postgres: string; mysql: string; sqlite: string }
  | { type: 'export_done'; path?: string }
  | { type: 'connector_status'; connectors: ConnectorStatus[] }
  | { type: 'ai_response'; content: string; done: boolean }
  | { type: 'error'; message: string };
