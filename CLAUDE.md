# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Vespertide is a Rust workspace for defining database schemas in JSON/YAML and generating migration plans and SQL from model diffs. It enables declarative schema management by comparing the current model state against a baseline reconstructed from applied migrations.

## Build and Test Commands

```bash
# Build the entire workspace
cargo build

# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p vespertide-core
cargo test -p vespertide-planner

# Format code
cargo fmt

# Lint (important: use all targets and features)
cargo clippy --all-targets --all-features

# Regenerate JSON schemas
cargo run -p vespertide-schema-gen -- --out schemas

# Run CLI commands (use -p vespertide-cli)
cargo run -p vespertide-cli -- init
cargo run -p vespertide-cli -- new user
cargo run -p vespertide-cli -- diff
cargo run -p vespertide-cli -- sql
cargo run -p vespertide-cli -- revision -m "message"
cargo run -p vespertide-cli -- status
cargo run -p vespertide-cli -- log
```

## Architecture

### Core Data Flow

1. **Schema Definition**: Users define tables in JSON files (`TableDef`) in the `models/` directory
2. **Baseline Reconstruction**: Applied migrations are replayed to rebuild the baseline schema
3. **Diffing**: Current models are compared against the baseline to compute changes
4. **Planning**: Changes are converted into a `MigrationPlan` with versioned actions
5. **SQL Generation**: Migration actions are translated into PostgreSQL SQL statements

### Crate Responsibilities

- **vespertide-core**: Data structures (`TableDef`, `ColumnDef`, `ColumnType`, `MigrationAction`, `MigrationPlan`, constraints, indexes)
  - `ColumnType` enum with `Simple(SimpleColumnType)` and `Complex(ComplexColumnType)` variants
  - `ColumnType::to_sql()` and `ColumnType::to_rust_type()` methods for type conversion
- **vespertide-planner**:
  - `schema_from_plans()`: Replays applied migrations to reconstruct baseline schema
  - `diff_schemas()`: Compares two schemas and generates migration actions
  - `plan_next_migration()`: Combines baseline reconstruction + diffing to create the next migration
  - `apply_action()`: Applies a single migration action to a schema (used during replay)
  - `validate_*()`: Validates schemas and migration plans
- **vespertide-query**: Converts `MigrationAction` → PostgreSQL SQL with bind parameters
  - Uses `ColumnType::to_sql()` method for SQL type conversion
- **vespertide-config**: Manages `vespertide.json` (models/migrations directories, naming case preferences)
- **vespertide-cli**: Command-line interface implementation
- **vespertide-exporter**: Exports schemas to other formats (e.g., SeaORM entities)
  - Uses `ColumnType::to_rust_type(nullable)` method for Rust type conversion
- **vespertide-schema-gen**: Generates JSON Schema files for validation
- **vespertide-macro**: Placeholder for future runtime migration executor

### Key Architectural Patterns

**Migration Replay Pattern**: The planner doesn't store a "current database state" - it reconstructs it by replaying all applied migrations in order. This ensures the baseline is always derivable from the migration history.

**Declarative Diffing**: Users declare the desired end state in model files. The diff engine compares this against the reconstructed baseline to compute necessary changes.

**Action-Based Migrations**: All changes are expressed as typed `MigrationAction` enums (CreateTable, AddColumn, ModifyColumnType, etc.) rather than raw SQL. SQL generation happens in a separate layer.

## Important Implementation Details

### ColumnDef Structure
When creating `ColumnDef` instances in tests or code, you must initialize ALL fields including the newer inline constraint fields:

```rust
ColumnDef {
    name: "id".into(),
    r#type: ColumnType::Simple(SimpleColumnType::Integer),
    nullable: false,
    default: None,
    comment: None,
    primary_key: None,      // Inline PK declaration
    unique: None,           // Inline unique constraint
    index: None,            // Inline index creation
    foreign_key: None,      // Inline FK definition
}
```

These inline fields (added recently) allow constraints to be defined directly on columns in addition to table-level `TableConstraint` definitions.

### ColumnType Structure
`ColumnType` is an enum with two variants:
- `Simple(SimpleColumnType)`: Built-in types like `Integer`, `Text`, `Boolean`, etc.
- `Complex(ComplexColumnType)`: Types with parameters like `Varchar { length }` or `Custom { custom_type }`

**Important**: In Rust code, always use `ColumnType::Simple(SimpleColumnType::Integer)` instead of the old `ColumnType::Integer` syntax. The `From` trait is implemented for convenience:
```rust
// These are equivalent:
ColumnType::Simple(SimpleColumnType::Integer)
SimpleColumnType::Integer.into()
```

### ColumnType Methods
`ColumnType` provides two utility methods:
- `to_sql()`: Returns the PostgreSQL SQL type string (e.g., `"INTEGER"`, `"VARCHAR(255)"`)
- `to_rust_type(nullable: bool)`: Returns the Rust type string for SeaORM entity generation (e.g., `"i32"` or `"Option<i32>"`)

These methods replace the old standalone functions `column_type_sql()` and `rust_type()`.

### Foreign Key Definition
Foreign keys can be defined inline on columns via the `foreign_key` field:

```rust
pub struct ForeignKeyDef {
    pub ref_table: TableName,
    pub ref_columns: Vec<ColumnName>,
    pub on_delete: Option<ReferenceAction>,
    pub on_update: Option<ReferenceAction>,
}
```

### Migration Plan Validation
- Non-nullable columns added to existing tables require either a `default` value or a `fill_with` backfill expression
- Schemas are validated for constraint consistency before diffing
- The planner validates that column/table names follow the configured naming case

### SQL Generation Target
All SQL generation currently targets **PostgreSQL only**. When modifying the query builder, ensure PostgreSQL compatibility.

### JSON Schema Generation
The `vespertide-schema-gen` crate uses `schemars` to generate JSON Schemas from the Rust types. After modifying core data structures, regenerate schemas with:
```bash
cargo run -p vespertide-schema-gen -- --out schemas
```

Schema base URL can be overridden via `VESP_SCHEMA_BASE_URL` environment variable.

## Testing Patterns

- Tests use helper functions like `col()` and `table()` to reduce boilerplate
- Use `rstest` for parameterized tests (common in planner/query crates)
- Use `serial_test::serial` for tests that modify the filesystem or working directory
- Snapshot testing with `insta` is used in the exporter crate

## Limitations

- YAML loading is not implemented (templates can be generated but not parsed)
- Runtime migration executor (`run_migrations`) in `vespertide-macro` is not implemented
- Only PostgreSQL SQL generation is supported
