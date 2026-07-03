# vespertide-query

Converts `MigrationAction` enums to SQL via sea-query intermediate representation.

## STRUCTURE

```
src/
├── lib.rs              # Re-exports: build_action_queries, BuiltQuery, DatabaseBackend
├── builder.rs          # build_plan_queries() - orchestrates full plan with schema evolution
├── error.rs            # QueryError enum
└── sql/
    ├── mod.rs          # build_action_queries() dispatch - matches all 14 MigrationAction variants (1507 lines)
    ├── types.rs        # BuiltQuery (11 variants), DatabaseBackend, RawSql
    ├── helpers.rs      # Column type mapping, FK actions, enum handling, naming
    ├── create_table.rs # build_create_table(), build_create_table_for_backend()
    ├── add_column.rs   # Temp table for SQLite non-nullable/enum columns
    ├── add_constraint.rs     # Constraint SQL generation (1356 lines)
    ├── remove_constraint.rs  # Complex SQLite temp table workarounds (1465 lines)
    ├── modify_column_*.rs    # type, nullable, default, comment handlers
    ├── rename_*.rs     # Simple ALTER statements
    ├── delete_column.rs # DROP COLUMN with SQLite rebuild (1084 lines)
    ├── delete_*.rs     # Other DROP statements
    └── raw_sql.rs      # RawSql emergency escape hatch
```

## WHERE TO LOOK

| Task | File | Key Function |
|------|------|--------------|
| Add new action handler | `sql/mod.rs` | Add to `build_action_queries()` match |
| Column type mapping | `sql/helpers.rs` | `apply_column_type_with_table()` |
| SQLite workarounds | `sql/remove_constraint.rs` | `{table}_temp` pattern |
| Backend-specific emergency SQL | `sql/types.rs` | `RawSql::per_backend()` |
| Default value conversion | `sql/helpers.rs` | `convert_default_for_backend()` |
| Enum type handling | `sql/helpers.rs` | `build_create_enum_type_sql()` |

## CONVENTIONS

```rust
// BuiltQuery wraps sea-query statements - call .build(backend) for SQL string
let query = BuiltQuery::CreateTable(Box::new(stmt));
let sql = query.build(DatabaseBackend::Postgres);

// Custom SQL without bind params - use Expr::cust()
col.default(Expr::cust("CURRENT_TIMESTAMP"));

// Emergency escape hatch only; prefer typed MigrationAction handlers
BuiltQuery::Raw(RawSql::per_backend(pg_sql, mysql_sql, sqlite_sql))

// SQLite temp table pattern (for ALTER limitations):
// 1. CREATE TABLE {table}_temp (new schema)
// 2. INSERT INTO {table}_temp SELECT ... FROM {table}
// 3. DROP TABLE {table}
// 4. ALTER TABLE {table}_temp RENAME TO {table}
// 5. Recreate indexes
```

## ANTI-PATTERNS

| Pattern | Why Bad |
|---------|---------|
| Direct SQL string building | Use sea-query builders, wrap in `BuiltQuery` |
| Using bind parameters | Not supported - use `Expr::cust()` for literals |
| Ignoring SQLite for constraints | SQLite needs temp table for PK/UNIQUE/FK/CHECK changes |
| Forgetting index recreation | After SQLite temp table rename, indexes are lost |
| Skipping `current_schema` param | Required for SQLite temp table to know column list |
| **PG-only / MySQL-only / SQLite-only test** | Every SQL-emit test in this crate MUST cover the full backend matrix (see TEST POLICY below) |
| **Orphan single-backend snapshot file** | A `*_postgres.snap` without sibling `*_mysql.snap` and `*_sqlite.snap` is a fault — either the test no longer fans out or the file is stale |

## TEST POLICY — N-BACKEND TRIPLE (MANDATORY)

Every test in `vespertide-query` that exercises SQL emission **MUST** assert
on every supported backend. Today that is **`{PG, MySQL, SQLite}`**, so each
test produces a **triple of snapshot files** sharing a base name and
differing only in the `_postgres` / `_mysql` / `_sqlite` suffix.

### Why mandatory

Per-backend divergence is the whole point of `vespertide-query`: SQLite needs
temp-table rebuilds where PG can do a one-line ALTER, MySQL has its own quirks
on `MODIFY COLUMN`, and so on. A single-backend test silently hides emit bugs
in the other two backends. Production users hit the broken path the moment
they switch the `--backend` flag.

### How to satisfy

Use one of the two canonical patterns below — never write a `#[test]` that
builds SQL against only one `DatabaseBackend`.

**Pattern A — `rstest` per-case (preferred for new tests):**
```rust
use rstest::rstest;

#[rstest]
#[case::postgres(DatabaseBackend::Postgres)]
#[case::mysql(DatabaseBackend::MySql)]
#[case::sqlite(DatabaseBackend::Sqlite)]
fn create_table_snapshot(#[case] backend: DatabaseBackend) {
    let sql = build_create_table(/* ... */).build(backend);
    insta::with_settings!(
        { snapshot_suffix => format!("create_table_{backend:?}") },
        { insta::assert_snapshot!(sql); }
    );
}
```

**Pattern B — `for backend in [..]` fan-out macro (used by
`sql/remap_enum_values.rs`):**
```rust
macro_rules! all_backends {
    ($name:ident, $fixture:expr) => {
        #[test]
        fn $name() {
            for (backend, tag) in [
                (DatabaseBackend::Postgres, "postgres"),
                (DatabaseBackend::MySql, "mysql"),
                (DatabaseBackend::Sqlite, "sqlite"),
            ] {
                let sql = run(backend, $fixture);
                with_settings!(
                    { snapshot_suffix => format!("{}_{}", stringify!($name), tag) },
                    { assert_snapshot!(sql); }
                );
            }
        }
    };
}
all_backends!(single_pair, &[(5_i64, 100_i64)]);
```

### Snapshot file naming

Each test emits exactly **N** files where N = number of supported backends
(currently 3). File suffixes are `_postgres.snap`, `_mysql.snap`,
`_sqlite.snap`. Adding a backend to the workspace requires extending every
existing test's case set or backend loop — no exceptions.

### Adding a new backend

When a new `DatabaseBackend` variant lands, the policy automatically
extends. The change must include:

1. Update `DatabaseBackend` in `sql/types.rs`.
2. Update every `#[case::backend(...)]` rstest in this crate.
3. Update every backend-loop macro (Pattern B) to include the new backend.
4. Re-run `cargo insta test --workspace && cargo insta accept` to generate
   the new snapshot rows.
5. Audit for **orphan snapshot files** with this PowerShell snippet
   (returns empty when policy holds):
   ```powershell
   $all = Get-ChildItem -Path crates\vespertide-query -Recurse -Filter "*.snap" |
       ForEach-Object {
           $m = [regex]::Match($_.BaseName, '^(.+)_(postgres|mysql|sqlite)$')
           if ($m.Success) {
               [PSCustomObject]@{ Dir = $_.DirectoryName; Base = $m.Groups[1].Value; Backend = $m.Groups[2].Value }
           }
       }
   $all | Group-Object Dir, Base | Where-Object {
       $b = ($_.Group | ForEach-Object Backend) | Sort-Object -Unique
       -not ($b -contains 'postgres' -and $b -contains 'mysql' -and $b -contains 'sqlite')
   }
   ```

## NOTES

- `build_action_queries()` is exhaustive for all 14 `MigrationAction` variants: CreateTable, DeleteTable, AddColumn, RenameColumn, DeleteColumn, ModifyColumnType, ModifyColumnNullable, ModifyColumnDefault, ModifyColumnComment, AddConstraint, RemoveConstraint, ReplaceConstraint, RenameTable, RawSql.
- Prefer typed `MigrationAction` enums; `RawSql` exists as a documented emergency escape hatch, but is not recommended for normal use.
- SQLite has full feature support via temp-table-rebuild workarounds for ALTER limitations.
- YAML and JSON are both fully supported upstream input formats.
- Every `.rs` file must stay ≤ 1000 lines (CI enforced); current hotspots are `sql/mod.rs` (1507), `remove_constraint.rs` (1465), `add_constraint.rs` (1356), and `delete_column.rs` (1084).
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
