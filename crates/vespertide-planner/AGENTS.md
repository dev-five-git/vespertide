# vespertide-planner

Schema diffing engine - compares baseline vs target schema to emit typed migration actions.

## STRUCTURE

```
src/
├── diff.rs      # 3200+ lines - Schema comparison, topological sort
├── validate.rs  # 1800+ lines - Schema/plan validation  
├── apply.rs     # 1400+ lines - Apply actions to in-memory schema
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
