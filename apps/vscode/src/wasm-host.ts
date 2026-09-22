/**
 * WASM host wrapper.
 *
 * Stubs are active until packages/wasm is built and linked.
 * To connect real WASM:
 *   1. Build: `cargo build -p vespertide-wasm --target wasm32-unknown-unknown`
 *   2. Run wasm-bindgen to produce JS glue + .wasm file
 *   3. Copy output into `dist/wasm/`
 *   4. Uncomment the import below and replace stub bodies
 */

// TODO: uncomment when packages/wasm is ready
// import init, { parse_orm, render_erd, convert_orm, generate_migration, svg_to_pdf }
//   from '../../packages/wasm/pkg';

import type { OrmType, DbDialect, Schema } from './messages';

let initialized = false;

export async function initWasm(_extensionPath: string): Promise<void> {
  if (initialized) return;
  // TODO: const wasmPath = path.join(_extensionPath, 'dist', 'wasm', 'vespertide_bg.wasm');
  // await init(wasmPath);
  initialized = true;
}

export async function parseOrm(source: string, orm: OrmType): Promise<Schema> {
  // TODO: return parse_orm(source, orm);
  return { _stub: true, orm, source } as Schema;
}

export async function renderErd(_schema: Schema): Promise<string> {
  // TODO: return render_erd(_schema);
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 220" style="max-width:100%">
  <rect width="400" height="220" rx="8" fill="none"
    stroke="currentColor" stroke-opacity="0.15" stroke-width="1"/>
  <rect x="40" y="40" width="140" height="80" rx="6"
    fill="none" stroke="currentColor" stroke-opacity="0.4" stroke-width="1.5"/>
  <text x="110" y="64" text-anchor="middle" font-family="monospace" font-size="12"
    fill="currentColor" font-weight="bold">User</text>
  <line x1="40" y1="72" x2="180" y2="72" stroke="currentColor" stroke-opacity="0.3"/>
  <text x="52" y="90" font-family="monospace" font-size="11" fill="currentColor" opacity="0.7">id: Int</text>
  <text x="52" y="108" font-family="monospace" font-size="11" fill="currentColor" opacity="0.7">name: String</text>
  <rect x="220" y="40" width="140" height="80" rx="6"
    fill="none" stroke="currentColor" stroke-opacity="0.4" stroke-width="1.5"/>
  <text x="290" y="64" text-anchor="middle" font-family="monospace" font-size="12"
    fill="currentColor" font-weight="bold">Post</text>
  <line x1="220" y1="72" x2="360" y2="72" stroke="currentColor" stroke-opacity="0.3"/>
  <text x="232" y="90" font-family="monospace" font-size="11" fill="currentColor" opacity="0.7">id: Int</text>
  <text x="232" y="108" font-family="monospace" font-size="11" fill="currentColor" opacity="0.7">userId: Int</text>
  <line x1="180" y1="80" x2="220" y2="80"
    stroke="currentColor" stroke-opacity="0.5" stroke-width="1.5" stroke-dasharray="4 2"/>
  <text x="200" y="170" text-anchor="middle" font-family="monospace" font-size="11"
    fill="currentColor" opacity="0.4">WASM 연결 전 미리보기</text>
</svg>`;
}

export async function convertOrm(source: string, from: OrmType, to: OrmType): Promise<string> {
  // TODO: return convert_orm(source, from, to);
  return `// [stub] ${from} → ${to} 변환\n// packages/wasm 연결 후 실제 변환이 동작합니다\n\n${source}`;
}

export async function generateMigration(_schema: Schema, _db: DbDialect): Promise<string> {
  // TODO: return generate_migration(_schema, _db);
  return '';
}

/** Returns null if WASM svg_to_pdf is not available (fallback handled in export.ts) */
export async function svgToPdf(_svg: string): Promise<Buffer | null> {
  // TODO: return Buffer.from(svg_to_pdf(_svg));
  return null;
}
