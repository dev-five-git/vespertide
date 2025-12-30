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
| `vespertide revision -m "<msg>"` | Persist pending changes as a migration file |
| `vespertide status` | Show configuration and sync status overview |
| `vespertide log` | List applied migrations with generated SQL |
| `vespertide export --orm seaorm` | Export models to SeaORM entity code |

## Supported Databases

Vespertide generates database-specific SQL for:

| Database | SQL Syntax | Notes |
|----------|------------|-------|
| PostgreSQL | `"identifier"` | Full feature support |
| MySQL | `` `identifier` `` | Full feature support |
| SQLite | `"identifier"` | Full feature support |

## ORM Export

Export your schema definitions to ORM-specific code:

```bash
# SeaORM (Rust)
vespertide export --orm seaorm

# SQLAlchemy (Python)
vespertide export --orm sqlalchemy

# SQLModel (Python - SQLAlchemy + Pydantic)
vespertide export --orm sqlmodel
```

| ORM | Language | Description |
|-----|----------|-------------|
| SeaORM | Rust | Async ORM for Rust with compile-time checked queries |
| SQLAlchemy | Python | Python SQL toolkit and ORM |
| SQLModel | Python | SQLAlchemy + Pydantic integration for FastAPI |

## Runtime Migrations (Macro)

Use the `vespertide_migration!` macro to run migrations at application startup. The macro generates database-specific SQL at compile time for zero-runtime overhead.

### Setup

Add dependencies to your `Cargo.toml`:

```toml
[dependencies]
vespertide = "0.1"
sea-orm = { version = "1.0", features = ["runtime-tokio-native-tls", "sqlx-postgres"] }
tokio = { version = "1", features = ["full"] }
```

### Usage

```rust
use sea_orm::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = Database::connect("postgres://user:pass@localhost/mydb").await?;

    // Run migrations (uses SeaORM connection)
    vespertide::vespertide_migration!(db).await?;

    Ok(())
}
```

### Options

```rust
// Custom version table name (default: "vespertide_version")
vespertide::vespertide_migration!(db, version_table = "my_migrations").await?;
```

The macro:
- Creates a version table if it doesn't exist
- Tracks applied migrations by version number
- Generates SQL for PostgreSQL, MySQL, and SQLite at compile time
- Automatically selects the correct SQL dialect based on your SeaORM connection

## Model Definition

Models are JSON files in the `models/` directory:

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "user",
  "columns": [
    { "name": "id", "type": "integer", "nullable": false, "primary_key": true },
    { "name": "email", "type": "text", "nullable": false, "unique": true, "index": true },
    { "name": "name", "type": "text", "nullable": false },
    { "name": "created_at", "type": "timestamptz", "nullable": false, "default": "NOW()" }
  ],
  "constraints": []
}
```

### Column Types

**Simple Types** (string values in JSON):
| Type | SQL Type |
|------|------------|
| `"integer"` | INTEGER |
| `"big_int"` | BIGINT |
| `"text"` | TEXT |
| `"boolean"` | BOOLEAN |
| `"timestamp"` | TIMESTAMP |
| `"timestamptz"` | TIMESTAMPTZ |
| `"uuid"` | UUID |
| `"jsonb"` | JSONB |
| `"small_int"` | SMALLINT |
| `"real"` | REAL |
| `"double_precision"` | DOUBLE PRECISION |
| `"date"` | DATE |
| `"time"` | TIME |
| `"bytea"` | BYTEA |
| `"json"` | JSON |
| `"inet"` | INET |
| `"cidr"` | CIDR |
| `"macaddr"` | MACADDR |

**Complex Types** (object values in JSON):
- `{ "kind": "varchar", "length": 255 }` → VARCHAR(255)
- `{ "kind": "custom", "custom_type": "DECIMAL(10,2)" }` → DECIMAL(10,2)
- `{ "kind": "custom", "custom_type": "UUID" }` → UUID

### Inline Constraints

Constraints can be defined directly on columns:

```json
{
  "name": "user_id",
  "type": "integer",
  "nullable": false,
  "foreign_key": {
    "ref_table": "user",
    "ref_columns": ["id"],
    "on_delete": "Cascade"
  },
  "index": true
}
```

See [SKILL.md](SKILL.md) for complete documentation on model definitions.

## Architecture

```
vespertide/
├── vespertide-core      # Data structures (TableDef, ColumnDef, MigrationAction)
├── vespertide-planner   # Schema diffing and migration planning
├── vespertide-query     # SQL generation
├── vespertide-config    # Configuration management
├── vespertide-cli       # Command-line interface
├── vespertide-exporter  # ORM code generation (SeaORM, SQLAlchemy, SQLModel)
├── vespertide-schema-gen # JSON Schema generation
└── vespertide-macro     # Compile-time migration macro for SeaORM
```

### How It Works

1. **Define Models**: Write table definitions in JSON files
2. **Replay Migrations**: Applied migrations are replayed to reconstruct the baseline schema
3. **Diff Schemas**: Current models are compared against the baseline
4. **Generate Plan**: Changes are converted into typed `MigrationAction` enums
5. **Emit SQL**: Migration actions are translated to SQL

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
# Build
cargo build

# Test
cargo test

# Lint
cargo clippy --all-targets --all-features

# Format
cargo fmt

# Regenerate JSON Schemas
cargo run -p vespertide-schema-gen -- --out schemas
```

## Limitations

- YAML loading is not yet implemented (templates can be generated but not parsed)

## License

Apache-2.0
