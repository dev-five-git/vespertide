---
name: vespertide
description: Define database schemas in JSON and generate migration plans. Use this skill when creating or modifying database models, defining tables with columns and inline constraints (primary_key, unique, index, foreign_key) for Vespertide-based projects.
---

# Vespertide Database Schema Definition

This skill helps you create and manage database models using Vespertide, a declarative schema management tool.

## Installation

```bash
cargo install vespertide-cli
```

## CLI Commands

```bash
vespertide init                    # Initialize project with vespertide.json
vespertide new <name>              # Create a new model template
vespertide diff                    # Show pending changes
vespertide sql                     # Preview SQL for pending migration
vespertide revision -m "message"   # Create migration file
vespertide status                  # Show project status
vespertide log                     # List applied migrations
```

## Model File Structure

Models are JSON files in the `models/` directory:

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "table_name",
  "columns": [],
  "constraints": []
}
```

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Table name (snake_case) |
| `columns` | array | Column definitions |
| `constraints` | array | Table-level constraints (can be empty `[]`) |

**Note**: The `indexes` field has been removed. Use inline `index` fields on columns instead (see Inline Constraints below).

## Column Definition

### Required Fields

```json
{
  "name": "column_name",
  "type": "ColumnType",
  "nullable": false
}
```

### Optional Fields

| Field | Type | Description |
|-------|------|-------------|
| `default` | string | Default value expression (e.g., `"NOW()"`, `"'pending'"`) |
| `comment` | string | Column description |
| `primary_key` | boolean | Inline primary key |
| `unique` | boolean \| string \| string[] | Inline unique constraint |
| `index` | boolean \| string \| string[] | Inline index |
| `foreign_key` | object | Inline foreign key definition |

## Column Types

Column types in JSON can be either simple (string) or complex (object) values.

### Simple Types (Built-in)

Simple types are represented as strings in JSON (snake_case):

| Type | SQL Type | Use Cases |
|------|------------|-----------|
| `"small_int"` | SMALLINT | Small integers (-32768 to 32767) |
| `"integer"` | INTEGER | IDs, counters |
| `"big_int"` | BIGINT | Large numbers |
| `"real"` | REAL | Single precision float |
| `"double_precision"` | DOUBLE PRECISION | Double precision float |
| `"text"` | TEXT | Strings |
| `"boolean"` | BOOLEAN | Flags |
| `"date"` | DATE | Date only |
| `"time"` | TIME | Time only |
| `"timestamp"` | TIMESTAMP | Date/time without timezone |
| `"timestamptz"` | TIMESTAMPTZ | Date/time with timezone |
| `"bytea"` | BYTEA | Binary data |
| `"uuid"` | UUID | UUIDs |
| `"json"` | JSON | JSON data |
| `"jsonb"` | JSONB | Binary JSON (indexable) |
| `"inet"` | INET | IPv4/IPv6 address |
| `"cidr"` | CIDR | Network address |
| `"macaddr"` | MACADDR | MAC address |

**Note**: In JSON, simple types are written as lowercase strings (e.g., `"integer"`, `"text"`). The Rust enum uses `SimpleColumnType` wrapped in `ColumnType::Simple()`.

### Complex Types

Complex types are represented as objects with a `kind` field:

**VARCHAR with length:**
```json
{ "kind": "varchar", "length": 255 }
```

**Custom types:**
```json
{ "kind": "custom", "custom_type": "DECIMAL(10,2)" }
{ "kind": "custom", "custom_type": "NUMERIC(20,8)" }
{ "kind": "custom", "custom_type": "INTERVAL" }
{ "kind": "custom", "custom_type": "UUID" }
```

**Note**: In Rust code, complex types are represented as `ColumnType::Complex(ComplexColumnType::Varchar { length })` or `ColumnType::Complex(ComplexColumnType::Custom { custom_type })`.

## Inline Constraints

### Primary Key

```json
{
  "name": "id",
  "type": "integer",
  "nullable": false,
  "primary_key": true
}
```

### Unique

```json
{ "name": "email", "type": "text", "nullable": false, "unique": true }
```

Named or composite unique:
```json
{ "name": "tenant_id", "type": "integer", "nullable": false, "unique": ["uq_tenant_user"] },
{ "name": "username", "type": "text", "nullable": false, "unique": ["uq_tenant_user"] }
```

### Index

```json
{ "name": "email", "type": "text", "nullable": false, "index": true }
```

Composite index:
```json
{ "name": "user_id", "type": "integer", "nullable": false, "index": ["idx_user_date"] },
{ "name": "created_at", "type": "timestamp", "nullable": false, "index": ["idx_user_date"] }
```

### Foreign Key

```json
{
  "name": "user_id",
  "type": "integer",
  "nullable": false,
  "foreign_key": {
    "ref_table": "user",
    "ref_columns": ["id"],
    "on_delete": "Cascade",
    "on_update": null
  },
  "index": true
}
```

Reference actions: `"Cascade"`, `"Restrict"`, `"SetNull"`, `"SetDefault"`, `"NoAction"`

## Table-Level Constraints

```json
"constraints": [
  { "type": "primary_key", "columns": ["id"] },
  { "type": "unique", "name": "uq_email", "columns": ["email"] },
  { "type": "foreign_key", "name": "fk_post_user", "columns": ["user_id"], "ref_table": "user", "ref_columns": ["id"], "on_delete": "Cascade" },
  { "type": "check", "name": "check_positive", "expr": "amount > 0" }
]
```

## Indexes

**Prefer inline indexes** on column definitions instead of table-level indexes:

```json
{
  "name": "email",
  "type": "text",
  "nullable": false,
  "index": true
}
```

For composite indexes, use the same index name on multiple columns:

```json
{ "name": "user_id", "type": "integer", "nullable": false, "index": ["idx_user_date"] },
{ "name": "created_at", "type": "timestamp", "nullable": false, "index": ["idx_user_date"] }
```

## Examples

### Basic User Table

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

### Post Table with Foreign Key

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "post",
  "columns": [
    { "name": "id", "type": "integer", "nullable": false, "primary_key": true },
    { "name": "user_id", "type": "integer", "nullable": false, "foreign_key": { "ref_table": "user", "ref_columns": ["id"], "on_delete": "Cascade" }, "index": true },
    { "name": "title", "type": "text", "nullable": false },
    { "name": "content", "type": "text", "nullable": false },
    { "name": "published", "type": "boolean", "nullable": false, "default": "false" },
    { "name": "created_at", "type": "timestamptz", "nullable": false, "default": "NOW()" }
  ],
  "constraints": []
}
```

### Order Table with Custom Types and Check Constraint

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "order",
  "columns": [
    { "name": "id", "type": "uuid", "nullable": false, "primary_key": true, "default": "gen_random_uuid()" },
    { "name": "customer_id", "type": "integer", "nullable": false, "foreign_key": { "ref_table": "customer", "ref_columns": ["id"], "on_delete": "Restrict" }, "index": true },
    { "name": "total_amount", "type": { "kind": "custom", "custom_type": "DECIMAL(10,2)" }, "nullable": false },
    { "name": "status", "type": "text", "nullable": false, "default": "'pending'" },
    { "name": "metadata", "type": "jsonb", "nullable": true },
    { "name": "created_at", "type": "timestamptz", "nullable": false, "default": "NOW()" }
  ],
  "constraints": [
    { "type": "check", "name": "check_total_positive", "expr": "total_amount >= 0" }
  ]
}
```

### Many-to-Many Join Table

```json
{
  "$schema": "https://raw.githubusercontent.com/dev-five-git/vespertide/refs/heads/main/schemas/model.schema.json",
  "name": "user_role",
  "columns": [
    { "name": "user_id", "type": "integer", "nullable": false, "primary_key": true, "foreign_key": { "ref_table": "user", "ref_columns": ["id"], "on_delete": "Cascade" } },
    { "name": "role_id", "type": "integer", "nullable": false, "primary_key": true, "foreign_key": { "ref_table": "role", "ref_columns": ["id"], "on_delete": "Cascade" }, "index": true },
    { "name": "assigned_at", "type": "timestamptz", "nullable": false, "default": "NOW()" }
  ],
  "constraints": []
}
```

## Guidelines

1. **Always include `$schema`** for IDE validation and autocompletion
2. **Always specify `nullable`** on every column
3. **Always include empty array** for `constraints` even if unused
4. **Prefer inline constraints** (`primary_key`, `unique`, `index`, `foreign_key`) over table-level definitions
5. **Use inline `index` on foreign key columns** for query performance (e.g., `"index": true`)
6. **Use named constraints** (especially CHECK) for easier management
7. **Naming conventions**:
   - Tables: `snake_case` (e.g., `user_role`)
   - Columns: `snake_case` (e.g., `created_at`)
   - Indexes: `idx_{table}_{columns}`
   - Unique constraints: `uq_{table}_{columns}`
   - Foreign keys: `fk_{table}_{ref_table}`
   - Check constraints: `check_{description}`
8. **Timestamp columns**:
   - `created_at`: `"default": "NOW()"`, `nullable: false`
   - `updated_at`: `nullable: true` (managed by application)
9. **Boolean defaults**: Use string format `"true"` or `"false"`
10. **Adding NOT NULL columns** to existing tables requires either a `default` value or `fill_with` in migration
