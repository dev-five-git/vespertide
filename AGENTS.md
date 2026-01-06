# VESPERTIDE KNOWLEDGE BASE

**Generated:** 2026-01-07T01:39:00+09:00
**Commit:** d6c2411
**Branch:** export-with-python

## OVERVIEW

Rust workspace for declarative database schema management. Define schemas in JSON, diff against migration history, generate typed actions and SQL.

## STRUCTURE

```
vespertide/
├── crates/
│   ├── vespertide-core/      # Data structures: TableDef, ColumnDef, MigrationAction
│   ├── vespertide-planner/   # Schema diffing, baseline reconstruction, validation
│   ├── vespertide-query/     # SQL generation (Postgres/MySQL/SQLite)
│   ├── vespertide-cli/       # CLI commands: init, diff, sql, revision, export
│   ├── vespertide-exporter/  # ORM codegen: SeaORM, SQLAlchemy, SQLModel
│   ├── vespertide-loader/    # Filesystem loading of models/migrations
│   ├── vespertide-config/    # vespertide.json configuration
│   ├── vespertide-macro/     # Compile-time migration macro
│   ├── vespertide-naming/    # Naming convention utilities
│   ├── vespertide-schema-gen/# JSON Schema generation
│   └── vespertide/           # Re-export crate (user-facing API)
├── examples/app/             # Example project with models/migrations
├── schemas/                  # Generated JSON Schemas for IDE support
└── CLAUDE.md                 # Detailed implementation guidance
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Core types (TableDef, ColumnDef) | `vespertide-core/src/schema/` | Start with `table.rs`, `column.rs` |
| Column type system | `vespertide-core/src/schema/column.rs` | `ColumnType::Simple/Complex` variants |
| Migration actions | `vespertide-core/src/action.rs` | 12 action variants, `MigrationPlan` struct |
| Schema diffing | `vespertide-planner/src/diff.rs` | **3215 lines** - topological sort for FK deps |
| SQL generation | `vespertide-query/src/sql/` | One file per action type |
| CLI commands | `vespertide-cli/src/commands/` | `cmd_*` functions |
| ORM export | `vespertide-exporter/src/{seaorm,sqlalchemy,sqlmodel}/` | Backend-specific generators |
| Compile-time macro | `vespertide-macro/src/lib.rs` | `vespertide_migration!` proc macro |

## DATA FLOW

```
JSON Models → load_models() → Vec<TableDef>
                                    ↓
Applied Migrations → schema_from_plans() → Baseline Schema
                                                ↓
                            diff_schemas() → Vec<MigrationAction>
                                                ↓
                            plan_next_migration() → MigrationPlan
                                                        ↓
                            build_action_queries() → Vec<BuiltQuery>
                                                        ↓
                            BuiltQuery.build(backend) → SQL String
```

## CONVENTIONS

### ColumnType Usage (CRITICAL)
```rust
// CORRECT - Always use wrapped variant
ColumnType::Simple(SimpleColumnType::Integer)
SimpleColumnType::Integer.into()

// WRONG - Old flat syntax
ColumnType::Integer  // Does not exist
```

### ColumnDef Initialization
ALL fields required including inline constraint fields:
```rust
ColumnDef {
    name, r#type, nullable, default, comment,
    primary_key: None,   // Must include
    unique: None,        // Must include  
    index: None,         // Must include
    foreign_key: None,   // Must include
}
```

### Naming
- Indexes: `ix_{table}__{columns}` or `ix_{table}__{name}`
- Unique: `uq_{table}__{columns}`
- Foreign keys: `fk_{table}__{columns}`

## ANTI-PATTERNS

| Pattern | Why Bad |
|---------|---------|
| `ColumnType::Integer` | Use `ColumnType::Simple(SimpleColumnType::Integer)` |
| Forgetting inline fields in ColumnDef | Will cause compile errors - 4 Option fields required |
| Raw SQL in migrations | Use typed `MigrationAction` enums |
| Skipping `normalize()` on TableDef | Inline constraints won't convert to table-level |
| Assuming YAML works | YAML loading NOT implemented (templates only) |

## COMMANDS

```bash
# Build/Test
cargo build
cargo test
cargo clippy --all-targets --all-features
cargo fmt

# CLI (always use -p vespertide-cli)
cargo run -p vespertide-cli -- init
cargo run -p vespertide-cli -- new <model>
cargo run -p vespertide-cli -- diff
cargo run -p vespertide-cli -- sql
cargo run -p vespertide-cli -- revision -m "message"
cargo run -p vespertide-cli -- export --orm seaorm

# Regenerate JSON schemas
cargo run -p vespertide-schema-gen -- --out schemas

# Snapshot testing
cargo insta test -p vespertide-exporter
cargo insta accept
```

## COMPLEXITY HOTSPOTS

| File | Lines | What |
|------|-------|------|
| `planner/src/diff.rs` | 3215 | Schema diffing with topological FK sort |
| `exporter/src/seaorm/mod.rs` | 2961 | SeaORM codegen with relation inference |
| `planner/src/validate.rs` | 1821 | Schema/migration validation |
| `core/src/schema/table.rs` | 1582 | Table normalization logic |
| `query/src/sql/remove_constraint.rs` | 1581 | SQLite temp table workarounds |

## TESTING

- `rstest` for parameterized tests
- `serial_test::serial` for filesystem tests
- `insta` for snapshot testing (exporter crate)
- Helper functions: `col()`, `table()` reduce boilerplate
- ~1289 tests across 53 files

## DATABASE BACKENDS

| Backend | Identifier Quoting | Notes |
|---------|-------------------|-------|
| PostgreSQL | `"identifier"` | Full feature support |
| MySQL | `` `identifier` `` | Full feature support |
| SQLite | `"identifier"` | Temp table workarounds for ALTER |

## NOTES

- Edition 2024 (bleeding edge)
- No LSP available - use grep/AST tools
- YAML loading not implemented
- Migration replay pattern: baseline always reconstructed from history
