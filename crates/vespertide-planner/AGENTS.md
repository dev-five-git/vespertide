# vespertide-planner

Schema diffing engine - compares baseline vs target schema to emit typed migration actions.

## STRUCTURE

```
src/
├── diff.rs      # 4739 lines, scheduled for split - Schema comparison, topological sort
├── validate.rs  # 2299 lines, scheduled for split - Schema/plan validation
├── apply.rs     # 1617 lines, scheduled for split - Apply actions to in-memory schema
├── schema.rs    # Replay migrations → baseline schema
├── plan.rs      # High-level planning API
└── error.rs     # PlannerError enum
```

## WHERE TO LOOK

| Task | File | Key Functions |
|------|------|---------------|
| Compare schemas | `diff.rs` | `diff_schemas()` |
| Replay migrations | `schema.rs` | `schema_from_plans()` |
| One-shot planning | `plan.rs` | `plan_next_migration()` |
| Apply single action | `apply.rs` | `apply_action()` |
| Validate schema | `validate.rs` | `validate_schema()`, `validate_migration_plan()` |
| FK dependency sort | `diff.rs` | `topological_sort_tables()`, `sort_delete_tables()` |

## ALGORITHM NOTES

**Diffing Flow:**
1. Normalize both schemas (inline constraints → table-level)
2. Use BTreeMaps for deterministic iteration order
3. Detect: deleted tables, modified columns, added columns, constraint changes
4. Topologically sort CreateTable by FK dependencies (Kahn's algorithm)
5. Reverse-sort DeleteTable (dependents deleted first)

**Topological Sort (Kahn's):**
- Build adjacency list from FK references
- Track in-degree (dependency count) per table
- Process zero-dependency tables first
- Detect cycles via incomplete result

**Normalization Critical:** Both schemas normalized before comparison so inline `unique: true` equals table-level `Unique { columns: [...] }`.

## ANTI-PATTERNS

| Pattern | Problem |
|---------|---------|
| Comparing without normalize | Inline vs table-level constraints won't match |
| Using HashMap in diff | Non-deterministic action ordering |
| Ignoring topological sort | FK constraint violations on CREATE/DELETE |
| Forgetting `fill_with` validation | NOT NULL columns without defaults fail |

## DATA-DEPENDENT FAULT COVERAGE

`validate/` is a **pure static analyzer**: no DB connection, no row inspection.
The following fault classes from the migration fault taxonomy are intentionally
**out of scope** because they require runtime data access against a populated
database:

| ID | Name | Why out of scope |
|---|---|---|
| F1 | NOT NULL on existing NULLs | Requires counting actual NULL rows in production data |
| F2 | UNIQUE with duplicates | Requires scanning existing rows for duplicate keys |
| F3 | FK with orphan rows | Requires cross-table check of existing rows against parent table |
| F4 | CHECK with violating rows | Requires evaluating the CHECK expression against every row |

These faults are **partially mitigated** by the `fill_with` requirement:
`find_missing_fill_with` and `find_missing_enum_fill_with` force the user to
declare *how* to backfill / map removed enum values before the migration is
allowed. Full runtime verification is delegated to the database engine at
`ADD CONSTRAINT` time, which will reject the migration if existing rows violate
the new invariant.

Statically analysable faults that **are** detected here:

| Helper | Fault | Category |
|---|---|---|
| `validate_schema` | Structural integrity (duplicate names, FK targets, etc.) | A |
| `validate_migration_plan` | Enum default / NOT NULL gating | A·B |
| `find_missing_fill_with` | Backfill strategy required (partial F1/F2/F3) | A |
| `find_missing_enum_fill_with` | Removed enum value remapping (partial F7) | B |
| `find_missing_fk_supporting_indexes` | F51 — FK without leading-prefix index | G |
| `find_constraint_drops_without_replacement` | F50 — Integrity-preserving constraint dropped | A |

## NOTES

- YAML and JSON are both fully supported for models and migrations.
- Prefer typed `MigrationAction` enums; `RawSql` exists as a documented emergency escape hatch, but is opaque to baseline replay and not recommended for normal use.
- Every `.rs` file must stay ≤ 1000 lines (CI enforced); current planner hotspots are `diff.rs` (4739), `validate.rs` (2299), and `apply.rs` (1617).
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
