# vespertide-exporter

ORM code generation from `TableDef` schemas → SeaORM (Rust), SQLAlchemy (Python), SQLModel (Python),
JPA (Java), GORM (Go), Django (Python).

## STRUCTURE

```
src/
├── lib.rs              # Re-exports all backends
├── orm.rs              # OrmExporter trait, Orm enum, dispatch functions
├── seaorm/mod.rs       # scheduled for split - Entity/Model/Relation generation
├── sqlalchemy/mod.rs   # scheduled for split - declarative_base models
├── sqlmodel/mod.rs     # scheduled for split - SQLModel + Pydantic models
├── jpa/mod.rs           # JPA entity generation
├── gorm/mod.rs          # GORM struct + gorm-tag generation; tests/ holds its own snapshot suite
├── django/mod.rs        # Django models.Model generation; snapshots/ holds its own snapshot suite
├── utils/python.rs     # Shared Python-target helpers (e.g. collect_composite_fks — used by
│                        # sqlalchemy AND django, since both target Python)
└── tests/               # Shared cross-ORM `orm_cases!` fixtures + snapshots/ (all six ORMs)
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Add new ORM backend | Implement `OrmExporter` trait in new module |
| Type mapping (Rust) | `ColumnType::to_rust_type(nullable)` in `vespertide-core` |
| Type mapping (Python) | `UsedTypes` struct in each Python backend |
| Relation inference | `relation_field_defs_with_schema()`, `infer_field_name_from_fk_column()` |
| FK chain resolution | `resolve_fk_target()` follows FKs through intermediate tables |
| Enum generation | `render_enum()` in each backend |

## BACKEND NOTES

### SeaORM (Rust)
- **Relation inference**: `creator_user_id` → field name `creator_user`, relation enum `CreatorUser`
- **FK chains**: Follows FK→FK chains to find ultimate target table
- **Multiple FKs**: Generates `relation_enum` attribute when table has multiple FKs to same target
- **Output**: Entity, Model, ActiveModel, Column enum, Relation enum
- **Config**: `SeaOrmExporterWithConfig` for `extra_model_derives`

### SQLAlchemy (Python)
- Uses `declarative_base()` pattern
- `UsedTypes` tracks imports: `sa_types`, `datetime_types`, `needs_uuid`, etc.
- Generates `relationship()` for FKs, `__table_args__` for composite constraints

### SQLModel (Python)
- SQLAlchemy + Pydantic integration (`SQLModel` base class)
- Uses `Field()` instead of `Column()` with Pydantic-style defaults
- Lighter import tracking (no `sa_types` - uses native Python types)
- `sa_column_kwargs` for SQLAlchemy-specific options

### GORM (Go)
- **Forward FK**: single-column FK → belongs-to struct field with a `gorm:"foreignKey:..."` tag;
  composite (multi-column) FK → single relation field via comma-separated
  `foreignKey:Col1,Col2;references:RefCol1,RefCol2` (a real GORM feature, unlike Django below)
- **Reverse (has-many)**: `find_reverse_relations()` scans the full schema for FKs pointing back at
  the table, including **self-referencing FKs** (a table referencing itself, e.g.
  `categories.parent_id -> categories.id`) — the self-ref case is named `Children` rather than a
  pluralized table name to avoid colliding with the struct's own name
- **No M2M/junction detection**: a junction table (composite-PK, 2+ FKs) is rendered as a plain
  has-many to the junction struct itself, not a dedicated M2M relation — same limitation Django had
  before this was added there; not yet closed for GORM
- **Config**: `GormExporterWithConfig` takes the *resolved* package name (a `&str`), not a `GormConfig` — callers get it from `VespertideConfig::gorm_package_name(export_dir)`, which uses an explicit `gorm.package_name` if set, otherwise infers one from the actual export directory's final path segment (sanitized to a valid Go identifier), falling back to `"models"`. The CLI passes the real write target (`--export-dir` override or `model_export_dir`), not the config's static default, since Go requires `package` to match the directory the files live in.
- **Tests**: own snapshot suite under `gorm/tests/` (not the shared `orm_cases!` fixtures until the
  Django/GORM cross-ORM wiring pass), split into `tests/mod.rs` (type mapping, snapshots) and
  `tests/relations.rs` (composite-FK + self-ref regression tests)

### Django (Python)
- Renders `models.Model` classes with a `class Meta` (`db_table`, `indexes`, `constraints`)
- **M2M junction detection**: `find_many_to_many_fields()` recognizes composite-PK, 2+ FK junction
  tables and emits `ManyToManyField(..., through=..., related_name="+")` on both sides; purely
  self-referential junctions are skipped rather than guessed at
- **Composite (multi-column) FK**: Django has no native multi-column FK field, so
  `collect_composite_fks` (from `utils/python.rs`, shared with SQLAlchemy) is used to emit a
  `# composite foreign key: (...) -> ref_table(...)` comment instead of silently dropping the
  relationship
- **`build_default()`**: only emits a bare (unquoted) SQL default when it parses as a numeric
  literal — an unrecognized bare constant (e.g. a named SQL constant) is omitted rather than
  emitted as an undefined Python name
- **PK kwarg**: `primary_key=True` is always emitted for the (non-composite) PK column, regardless
  of field type — `models.AutoField`/`SmallAutoField`/`BigAutoField` do **not** imply
  `primary_key=True` in real Django; omitting it fails Django's own `fields.E100` system check
- **Config**: `DjangoExporterWithConfig` for `app_label` (omitted from `Meta` when unset)
- **Tests**: own snapshot suite under `django/snapshots/` (via inline `#[cfg(test)] mod tests` in
  `django/mod.rs`)

## TESTING

```bash
# Run all exporter tests
cargo test -p vespertide-exporter

# Update snapshots after changes
cargo insta test -p vespertide-exporter
cargo insta accept
```

- Snapshot testing with `insta` crate (YAML format)
- `rstest` for parameterized tests across all ORM backends
- Shared cross-ORM suite (`src/tests/snapshots/`, driven by the `orm_cases!` macro in
  `src/tests/mod.rs`): 362 snapshots across all six ORMs. Plus module-local suites for backend
  behavior that doesn't fit the shared fixture shape: `gorm/tests/snapshots/`,
  `django/snapshots/`, and per-backend snapshots under `seaorm/`, `sqlalchemy/`, `sqlmodel/`,
  `jpa/`.

## NOTES

- YAML and JSON are both fully supported input formats; exporter tests also use YAML-formatted insta snapshots.
- Generated ORM files are outputs only; edit Vespertide models, then regenerate.
- Every `.rs` file must stay ≤ 1000 lines (CI enforced); current hotspots include SeaORM, SQLAlchemy, SQLModel, and JPA backends.
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
