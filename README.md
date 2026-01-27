# Vespertide

Declarative database schema management. Define your schemas in JSON, and Vespertide automatically generates migration plans and SQL from model diffs.

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![GitHub Actions](https://img.shields.io/github/actions/workflow/status/dev-five-git/vespertide/CI.yml?branch=main&label=CI)](https://github.com/dev-five-git/vespertide/actions)
[![Codecov](https://img.shields.io/codecov/c/github/dev-five-git/vespertide)](https://codecov.io/gh/dev-five-git/vespertide)
[![Crates.io](https://img.shields.io/crates/v/vespertide-cli.svg)](https://crates.io/crates/vespertide-cli)

## Features

- **Declarative Schema**: Define your desired database state in JSON files
- **Automatic Diffing**: Vespertide compares your models against applied migrations to compute changes
- **Migration Planning**: Generates typed migration actions (not raw SQL) for safety and portability
- **Multi-Database Support**: PostgreSQL, MySQL, SQLite
- **Enum Types**: Native string enums and integer enums (no migration needed for new values)
- **Zero-Runtime Migrations**: Compile-time macro generates database-specific SQL
- **JSON Schema Validation**: Ships with JSON Schemas for IDE autocompletion and validation
- **ORM Export**: Export schemas to SeaORM, SQLAlchemy, SQLModel

## Installation

```bash
cargo install vespertide-cli
```

## Quick Start

```bash
# Initialize a new project
vespertide init

# Create a model template
vespertide new user

# Edit models/user.json, then check changes
vespertide diff

# Preview the SQL
vespertide sql

# Generate a migration file
vespertide revision -m "create user table"
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `vespertide init` | Create `vespertide.json` configuration file |
| `vespertide new <name>` | Create a new model template with JSON Schema reference |
| `vespertide diff` | Show pending changes between migrations and current models |
| `vespertide sql` | Print SQL statements for the next migration |
| `vespertide sql --backend mysql` | SQL for specific backend (postgres/mysql/sqlite) |
| `vespertide revision -m "<msg>"` | Persist pending changes as a migration file |
| `vespertide status` | Show configuration and sync status overview |
| `vespertide log` | List applied migrations with generated SQL |
| `vespertide export --orm seaorm` | Export models to ORM code |

## Model Definition

Models are JSON files in the `models/` directory. Always include `$schema` for IDE validation:

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "user",
  "columns": [
    { "name": "id", "type": "integer", "nullable": false, "primary_key": true },
    { "name": "email", "type": "text", "nullable": false, "unique": true, "index": true },
    { "name": "name", "type": { "kind": "varchar", "length": 100 }, "nullable": false },
    { 
      "name": "status", 
      "type": { "kind": "enum", "name": "user_status", "values": ["active", "inactive", "banned"] },
      "nullable": false,
      "default": "'active'"
    },
    { "name": "created_at", "type": "timestamptz", "nullable": false, "default": "NOW()" }
  ]
}
```

### Column Types

**Simple Types:**

| Type | SQL Type | Type | SQL Type |
|------|----------|------|----------|
| `"integer"` | INTEGER | `"text"` | TEXT |
| `"big_int"` | BIGINT | `"boolean"` | BOOLEAN |
| `"small_int"` | SMALLINT | `"uuid"` | UUID |
| `"real"` | REAL | `"json"` | JSON |
| `"double_precision"` | DOUBLE PRECISION | `"jsonb"` | JSONB |
| `"date"` | DATE | `"bytea"` | BYTEA |
| `"time"` | TIME | `"inet"` | INET |
| `"timestamp"` | TIMESTAMP | `"cidr"` | CIDR |
| `"timestamptz"` | TIMESTAMPTZ | `"macaddr"` | MACADDR |
| `"interval"` | INTERVAL | `"xml"` | XML |

**Complex Types:**

```json
{ "kind": "varchar", "length": 255 }
{ "kind": "char", "length": 2 }
{ "kind": "numeric", "precision": 10, "scale": 2 }
{ "kind": "enum", "name": "status", "values": ["active", "inactive"] }
{ "kind": "custom", "custom_type": "TSVECTOR" }
```

### Enum Types (Recommended)

Use enums instead of text columns for status fields and categories:

**String Enum** (PostgreSQL native enum):
```json
{
  "name": "status",
  "type": { "kind": "enum", "name": "order_status", "values": ["pending", "shipped", "delivered"] },
  "nullable": false,
  "default": "'pending'"
}
```

**Integer Enum** (stored as INTEGER, no DB migration needed for new values):
```json
{
  "name": "priority",
  "type": {
    "kind": "enum",
    "name": "priority_level",
    "values": [
      { "name": "low", "value": 0 },
      { "name": "medium", "value": 10 },
      { "name": "high", "value": 20 }
    ]
  },
  "nullable": false,
  "default": 10
}
```

### Inline Constraints (Preferred)

Define constraints directly on columns instead of using table-level `constraints`:

```json
{
  "name": "author_id",
  "type": "integer",
  "nullable": false,
  "foreign_key": {
    "ref_table": "user",
    "ref_columns": ["id"],
    "on_delete": "cascade"
  },
  "index": true
}
```

**Reference Actions** (snake_case): `"cascade"`, `"restrict"`, `"set_null"`, `"set_default"`, `"no_action"`

**Composite Primary Key** (inline):
```json
{ "name": "user_id", "type": "integer", "nullable": false, "primary_key": true },
{ "name": "role_id", "type": "integer", "nullable": false, "primary_key": true }
```

**Table-level constraints** are only needed for CHECK expressions:
```json
"constraints": [
  { "type": "check", "name": "check_positive", "expr": "amount > 0" }
]
```

See [SKILL.md](SKILL.md) for complete documentation.

## Migration Files

> **Important**: Migration files are auto-generated. Never create or edit them manually.

```bash
# Always use the CLI to create migrations
vespertide revision -m "add status column"
```

The only exception is adding `fill_with` values when prompted (for NOT NULL columns without defaults).

## Supported Databases

| Database | Identifier Quoting | Notes |
|----------|-------------------|-------|
| PostgreSQL | `"identifier"` | Full feature support |
| MySQL | `` `identifier` `` | Full feature support |
| SQLite | `"identifier"` | Full feature support |

## ORM Export

```bash
vespertide export --orm seaorm      # Rust - SeaORM entities
vespertide export --orm sqlalchemy  # Python - SQLAlchemy models
vespertide export --orm sqlmodel    # Python - SQLModel (FastAPI)
```

## Runtime Migrations (Macro)

Use the `vespertide_migration!` macro to run migrations at application startup:

```toml
[dependencies]
vespertide = "0.1"
sea-orm = { version = "2.0.0-rc", features = ["sqlx-postgres", "runtime-tokio-native-tls", "macros"] }
```

```rust
use sea_orm::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = Database::connect("postgres://user:pass@localhost/mydb").await?;
    vespertide::vespertide_migration!(db).await?;
    Ok(())
}
```

The macro generates database-specific SQL at compile time for zero-runtime overhead.

## Architecture

```
vespertide/
├── vespertide-core      # Data structures (TableDef, ColumnDef, MigrationAction)
├── vespertide-planner   # Schema diffing and migration planning
├── vespertide-query     # SQL generation (PostgreSQL, MySQL, SQLite)
├── vespertide-cli       # Command-line interface
├── vespertide-exporter  # ORM code generation
├── vespertide-macro     # Compile-time migration macro
└── vespertide-config    # Configuration management
```

### How It Works

1. **Define Models**: Write table definitions in JSON files with `$schema` for validation
2. **Replay Migrations**: Applied migrations are replayed to reconstruct the baseline schema
3. **Diff Schemas**: Current models are compared against the baseline
4. **Generate Plan**: Changes are converted into typed `MigrationAction` enums
5. **Emit SQL**: Migration actions are translated to database-specific SQL

## Configuration

`vespertide.json`:

```json
{
  "modelsDir": "models",
  "migrationsDir": "migrations",
  "tableNamingCase": "snake",
  "columnNamingCase": "snake",
  "modelFormat": "json"
}
```

## Development

```bash
cargo build                              # Build
cargo test                               # Test
cargo clippy --all-targets --all-features # Lint
cargo fmt                                # Format
cargo run -p vespertide-schema-gen -- --out schemas  # Regenerate JSON Schemas
```

## License

Apache-2.0
