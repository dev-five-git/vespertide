# VESPERTIDE-CORE

Core data structures for schema definition and migration planning.

## STRUCTURE

```
src/
├── lib.rs              # Re-exports all public types
├── action/             # MigrationAction (15 variants), MigrationPlan
│   ├── mod.rs          #   Variant definitions + plan struct (937 lines)
│   ├── display.rs      #   Human-readable rendering of actions
│   ├── prefix.rs       #   Table-name prefixing (uses TableName::with_prefix)
│   ├── narrowing_strategy.rs # Type-narrowing strategy enum + helpers
│   └── remap_mapping_serde.rs # RemapEnumValues mapping (de)serialization
├── migration.rs        # MigrationError, MigrationOptions
└── schema/
    ├── column.rs       # ColumnDef, ColumnType, SimpleColumnType, ComplexColumnType
    ├── table/          # TableDef + normalize()
    │   ├── mod.rs      #   TableDef definition + re-exports (49 lines)
    │   ├── normalize.rs#   Inline→table-level constraint conversion
    │   ├── normalize_proptest.rs # Property-based tests for normalize()
    │   └── validation.rs # Structural validation (duplicate columns, etc.)
    ├── constraint.rs   # TableConstraint (5 variants)
    ├── foreign_key.rs  # ForeignKeySyntax for inline FK definition
    ├── primary_key.rs  # PrimaryKeySyntax, PrimaryKeyDef
    ├── reference.rs    # ReferenceAction (Cascade, SetNull, etc.)
    ├── index.rs        # IndexDef
    ├── names.rs        # TableName, ColumnName, IndexName (String newtypes)
    └── str_or_bool.rs  # StringOrBool, StrOrBoolOrArray, DefaultValue
```

## WHERE TO LOOK

| Task | File | Key Items |
|------|------|-----------|
| Column type system | `schema/column.rs` | `ColumnType::Simple/Complex`, `to_rust_type()` |
| Table normalization | `schema/table.rs` | `TableDef.normalize()` - inline to table-level |
| Migration actions | `action.rs` | `MigrationAction` enum, `MigrationPlan` struct |
| Constraint types | `schema/constraint.rs` | `TableConstraint` (PK, Unique, FK, Check, Index) |

## TYPE PATTERNS

```rust
// ColumnType - ALWAYS use wrapped variants
ColumnType::Simple(SimpleColumnType::Integer)
ColumnType::Complex(ComplexColumnType::Varchar { length: 255 })

// ColumnDef - ALL fields required (4 inline constraint Options)
ColumnDef {
    name: "email".into(),
    r#type: ColumnType::Simple(SimpleColumnType::Text),
    nullable: false,
    default: None,
    comment: None,
    primary_key: None,   // Required
    unique: None,        // Required
    index: None,         // Required
    foreign_key: None,   // Required
}

// TableDef.normalize() - converts inline constraints to TableConstraint
let normalized = table_def.normalize()?;

// MigrationAction - tagged enum for SQL generation
MigrationAction::CreateTable { table, columns, constraints }
MigrationAction::AddColumn { table, column, fill_with }
```

## ANTI-PATTERNS

| Wrong | Correct |
|-------|---------|
| `ColumnType::Integer` | `ColumnType::Simple(SimpleColumnType::Integer)` |
| Omitting inline fields in ColumnDef | Include all 4: `primary_key`, `unique`, `index`, `foreign_key` |
| Using TableDef without normalize() | Call `normalize()` before diffing |
| Direct TableConstraint in column | Use inline syntax, let normalize() convert |

## NOTES

- Model and migration serialization supports both JSON and YAML.
- Prefer typed `MigrationAction` enums; `MigrationAction::RawSql` exists as a documented emergency escape hatch, but is not recommended for normal use.
- Shared proptest strategies live behind the `arbitrary` feature. Run property tests with `cargo test -p vespertide-core --features arbitrary`.
- Every production-only `.rs` file must stay ≤ 1000 lines, files carrying inline `#[cfg(test)] mod tests` ≤ 1200 lines (CI enforced via `scripts/check-line-budget.sh`); the current `action/mod.rs` (937 lines) is the near-ceiling file in this crate. `action.rs` and `schema/table.rs` were already split into the directories shown above.
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
