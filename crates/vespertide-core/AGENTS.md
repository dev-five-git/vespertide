# VESPERTIDE-CORE

Core data structures for schema definition and migration planning.

## STRUCTURE

```
src/
├── lib.rs              # Re-exports all public types
├── action.rs           # MigrationAction (14 variants), MigrationPlan (1236 lines; scheduled split)
├── migration.rs        # MigrationError, MigrationOptions
└── schema/
    ├── column.rs       # ColumnDef, ColumnType, SimpleColumnType, ComplexColumnType
    ├── table.rs        # TableDef, normalize() method
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
- Every `.rs` file must stay ≤ 1000 lines (CI enforced); current hotspots include `action.rs` (1236 lines) and `schema/table.rs` (1526 lines).
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
