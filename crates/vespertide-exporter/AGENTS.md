# vespertide-exporter

ORM code generation from `TableDef` schemas → SeaORM (Rust), SQLAlchemy (Python), SQLModel (Python), JPA (Java), Prisma (schema.prisma), Drizzle (TypeScript), GORM (Go), Django (Python).

## STRUCTURE

```
src/
├── lib.rs              # Re-exports all backends
├── orm.rs              # OrmExporter trait, Orm enum (SeaOrm/SqlAlchemy/SqlModel/Jpa/Prisma/Drizzle/Gorm/Django),
│                       #   Orm::file_extension(), dispatch
├── constraint_scan.rs  # Shared constraint scans + FK relation naming
│                       #   (single_column_fk_details/junction_targets/fk_relation_names/relation_segment/
│                       #   collect_back_relations)
├── enum_scan.rs        # Shared enum-column scans (Prisma/Drizzle/GORM/Django)
├── parallel_config.rs  # Rayon parallelism thresholds
├── python_naming.rs    # Shared PascalCase naming (SQLAlchemy/SQLModel/JPA/Django/GORM/CLI)
├── scope_names.rs      # Top-level names claimed once per schema (GORM package / Django module)
├── seaorm/             # mod.rs, render.rs, types.rs, enums.rs, imports.rs,
│                       #   relations/ (fk_resolve, naming, self_ref, reverse), tests/
├── sqlalchemy/         # mod.rs, render.rs, types.rs, enums.rs — declarative_base models
├── sqlmodel/           # mod.rs, render.rs, types.rs, enums.rs — SQLModel + Pydantic models
├── jpa/                # mod.rs, render.rs, types.rs — JPA/Hibernate entities
├── prisma/             # mod.rs, render.rs, types.rs, enums.rs — schema.prisma models
├── drizzle/            # mod.rs, render.rs, types.rs, enums.rs — Drizzle TypeScript models
├── gorm/               # mod.rs, render.rs, types.rs, enums.rs — GORM structs
├── django/             # mod.rs, render.rs, types.rs, enums.rs — Django models.Model classes
├── utils/              # common.rs (join_quoted/string_literal/unquote/claim_field_name/claim_binding/collect_composite_fks/is_jsonb_custom_type),
│                       #   python.rs (render_enum/enum_member_name/unmangled/column_type_to_python),
│                       #   typescript.rs (ts_binding)
└── tests/              # Shared orm_cases! cross-ORM snapshot suite + fixtures/ + snapshots/
```

Identifier escaping is centralized in `vespertide-naming`: `sanitize_identifier`
with `IdentifierStart::Underscore` (Java, SQLAlchemy, Django fields and choices
classes, ERD) or `IdentifierStart::Letter` (SeaORM, SQLModel/Pydantic, Prisma,
Drizzle, Django model classes, and GORM, which also upper-cases the first letter
because Go exports by case), plus
`seaorm_module_name` and `to_screaming_snake_case`. A backend that renames an
identifier MUST also emit the original database name (`@map`, `column_name`,
SQLAlchemy's positional column name).

Python keywords are escaped by `utils/python.rs::escape_python_keyword` (PEP 8's trailing
`_`) in SQLAlchemy and SQLModel. Django cannot use that form — fields.E001 forbids a
trailing `_` — so `django/render.rs::django_field_name` applies Django's field checks
(no `__`, no trailing `_`, not a keyword, not one of the model's own attributes in
`MODEL_ATTRIBUTES` — `pk`, `save`, `check`, `objects`, `Meta`, …) with `inspectdb`'s `_field`
repairs.

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

### JPA (Java)
- Jakarta Persistence (`jakarta.persistence.*`) entity classes with `@Entity`/`@Table`/`@Column`
- Enum types render as Java `enum` + `@Enumerated`
- FK columns render as `@ManyToOne`/`@JoinColumn` relations

### GORM (Go)
- **Forward FK**: single-column FK → belongs-to struct field with a `gorm:"foreignKey:..."` tag;
  composite (multi-column) FK → single relation field via comma-separated
  `foreignKey:Col1,Col2;references:RefCol1,RefCol2`. A single-column key names `references:` too
  when it points at anything but the target's primary key, which is what GORM would assume.
  The field is always a pointer (`*User`), nullable or not: held by value, a struct could not
  reference itself or a struct that references it back (`invalid recursive type`)
- **Reverse (has-one / has-many)**: built on the shared `constraint_scan::collect_back_relations`,
  so composite FKs get a reverse side and a one-to-one — a key that is the source's whole
  primary key, or that a unique covers exactly — renders as `*T` under the source struct's
  name instead of `[]T` under its plural. Tags mirror the forward side. A **self-referencing FK**
  (e.g. `categories.parent_id -> categories.id`) is named `Children` rather than a pluralized
  table name to avoid colliding with the struct's own name; names that would repeat gain a
  `By{key fields}` suffix (`SettingsByCreatedByUserID`)
- **No M2M/junction detection**: a junction table (composite-PK, 2+ FKs) is rendered as a plain
  has-many to the junction struct itself, not a dedicated M2M relation
- **Identifiers**: every struct, field and type name is an exported Go name (`exported_go_name`:
  `1users` → `X1users`), `Id` becomes `ID` only where it ends a word (`UserID`, but `Identity`),
  and one taken set per struct covers the columns first and then every relation field, so a
  has-many or belongs-to never takes a column's name (`Posts2`, `OrderRegions3`). That set starts
  with `TableName`, the method every struct gets (a `table_name` column becomes `TableName2`, and
  so does the belongs-to of a `table_name_id` key)
- **Package scope**: structs, enum types and enum constants all live in one Go package, so
  `scope_names::ScopeNames` claims them once for the whole schema — structs first, then enum
  types (bare while nothing else holds the identifier, otherwise `{Struct}{Enum}`), then
  constants (`{Type}{Variant}`; values that fold onto one name are numbered). A table `role` next
  to an enum `role`, or `Status` + `code` next to a `status_code` table, no longer redeclares.
  A single-table render claims the same way over `scope_names::scope_of` — the schema when it
  holds the table, the table alone otherwise
- **Tags**: `index:`/`uniqueIndex:` names come from the naming builders, so they match what the
  SQL layer creates and GORM groups a composite index by them; `char(N)` and the PG network types
  carry an explicit `type:`; a default GORM's tag syntax cannot hold (`;`, a function call)
  is omitted, an integer enum's variant-name default becomes its value, and a string field's
  default loses the doubled SQL quote (`'it''s'` → `'it's'`): GORM reads it as the value, while
  every other field's default stays the SQL it is. GORM trims every quote off both ends of that
  value, so a string default that starts or ends with `'` or `"` is omitted too. `struct_tag` quotes
  each tag value as the Go string `reflect.StructTag` reads, so a `"` or `\` in a column name or
  default is escaped, and the whole tag is an interpreted string when a value holds a backtick
- **Package name**: there is no `gorm` config section. `GormExporterWithConfig::for_export_dir`
  derives it from the directory the file is written to (`go_package_name`) — the export
  directory's final path segment sanitized into a Go identifier, falling back to `"models"`. The
  CLI passes the real write target (`--export-dir` override or `model_export_dir`) because Go
  expects `package` to name the directory the file lives in.
- **One file**: `GormExporterWithConfig::export` renders the whole schema as one source file, and
  that is what the CLI writes (`models.go`). A Go directory is one package and a relation is
  rendered from both of its ends, so models spread over directories would import each other in
  a cycle
- **Layout**: `gofmt_layout` is the last step of every render — tab indents, struct-field and
  constant columns padded the way `gofmt` aligns them, single blank lines — and `render_header`
  lists each import group in sorted order, so the file passes a project's `gofmt -l` check as
  written
- **Tests**: rendered output is pinned by the shared `orm_cases!` suite; the inline
  `#[cfg(test)] mod tests` blocks hold only function-level unit tests (`types.rs` Go type
  mapping; `render.rs` field and relation naming, package-scope constants, struct-tag escaping,
  default tags; `mod.rs` package-name inference)

### Django (Python)
- **Module scope**: model classes and choices classes share one module, so they are claimed
  through the same `scope_names::ScopeNames` (models first; a choices class is bare while
  nothing else holds the identifier, otherwise `{Model}{Enum}`). Members are scoped to their
  class and numbered there when two values fold onto one name — Python's `Enum` refuses a
  repeated member at import time. A choices class or member led by `__` keeps a single `_`
  (`utils/python.rs::unmangled`): Python mangles such a name inside a class body, so the member
  would be no member and the model could not name the class
- Renders `models.Model` classes with a `class Meta` (`managed = False` — vespertide owns the DDL,
  so `makemigrations` must not create or alter these tables — `db_table`, `indexes`, `constraints`).
  `UniqueConstraint` names come from `build_unique_constraint_name` with the source name as the
  key, matching the SQL layer; `Meta.indexes` use `build_index_name` the same way while the
  result fits Django's 30-character cap on index names (models.E034), and carry no `name=`
  past it — Django never creates the index of an unmanaged model, so its own name will do.
  The built name is `ix_{table}__{key}`, so a long table name reaches the cap on its own and
  even a short source name then goes unnamed
- **JSONB**: a `Custom` column type spelled `jsonb` maps to `models.JSONField` (the shared
  `is_jsonb_custom_type`); other custom types fall back to `TextField`
- **M2M junction detection**: `constraint_scan::junction_targets` (shared with SeaORM) recognizes
  composite-PK, 2+ FK junction tables; each side gets `ManyToManyField(..., through=...,
  related_name="+")`, named after the pluralized target (`{target}_via_{junction}` when two
  junctions reach one target) and run through `django_field_name` after the columns, so it never
  shadows a scalar field. Purely self-referential junctions are skipped rather than guessed at,
  and so is a junction that reaches either end by a composite key: that key renders as a
  comment, a `through` model needs a real `ForeignKey` to both ends (fields.E336), and Django
  cannot relate to the composite-key model it points at (fields.E347)
- **Names and actions Django's checks reject**: a model class never starts with `_` (models.E023;
  `1users` → `x1users`, the same letter escape SQLModel uses), and `on_delete=SET_DEFAULT` is only
  emitted when the FK column has a default, which then renders as `default=`; without one it
  falls back to `DO_NOTHING` (fields.E321), and so does `SET_NULL` on a field that is not null
  (fields.E320) — the table is unmanaged, so the database keeps applying its own rule
  (`types.rs::on_delete_for`)
- **Composite (multi-column) FK**: Django has no native multi-column FK field, so
  `collect_composite_fks` (from `utils/common.rs`, shared with SQLAlchemy, SQLModel and GORM)
  emits a `# composite foreign key: (...) -> ref_table(...)` comment instead of silently dropping
  the relationship
- **`build_default()`**: only emits a bare (unquoted) SQL default when it parses as a numeric
  literal — an unrecognized bare constant (e.g. a named SQL constant) is omitted rather than
  emitted as an undefined Python name. A quoted default becomes the Python string it spells
  (`'it''s'` → `"it's"`), and a `JSONField` gets none: Django wants a callable there
  (fields.E010), and the SQL literal is the document's text rather than its value
- **PK kwarg**: `primary_key=True` is always emitted for the (non-composite) PK column, regardless
  of field type — `models.AutoField`/`SmallAutoField`/`BigAutoField` do **not** imply
  `primary_key=True` in real Django; omitting it fails Django's own `fields.E100` system check
- **FK fields**: a FK column that is the table's (non-composite) PK or carries a single-column
  unique renders as `models.OneToOneField` — `ForeignKey(unique=True)` is only fields.W342 pointing
  at that class — and the PK one keeps `primary_key=True`. A key that references anything but
  the target's primary key carries `to_field=` (Django would otherwise join on the primary key
  and silently return the wrong rows), named through the target's own `column_field_names`. A
  key into a model with a composite primary key, or onto a column that is neither the target's
  primary key nor unique on its own, stays a plain column plus a
  `# foreign key: (col) -> table(ref)` comment: Django cannot relate to such a model
  (fields.E347) or through such a field (fields.E311). A key claims its attname along with its
  field name (`claim_relation_field_name`): Django stores it under `{field}_id`, which a plain
  column of that name would clash with (models.E006)
- **Config**: `DjangoExporterWithConfig` for `app_label` (omitted from `Meta` when unset); its
  `export` renders the whole schema as one module, which is what the CLI writes (`models.py`) —
  Django loads an app's models from its one `models` module
- **Tests**: rendered output is pinned by the shared `orm_cases!` suite; the inline
  `#[cfg(test)] mod tests` blocks hold only function-level unit tests (`mod.rs` field-class,
  `on_delete` and string-default mappings; `render.rs` field-name repairs, attname claims,
  relatable keys, choices-class names; `enums.rs` member numbering)

### Prisma (schema.prisma)
- Emits models only — no `datasource`/`generator` block, so the output drops into an existing schema
- Backend-neutral: no provider-specific `@db.*` native attributes are emitted
- `render_schema` is a Prisma-only single-file entry point that deduplicates enums globally,
  so its snapshot tests live inline in the module (still writing into `src/tests/snapshots/`)
- Renamed identifiers carry `@map` / `@@map`; enum members go through
  `to_screaming_snake_case` + `sanitize_identifier(IdentifierStart::Letter)`

### Drizzle (TypeScript)
- No backend-neutral form exists (`pgTable`/`mysqlTable`/`sqliteTable` fork at the
  `import` line), so one export writes one file per dialect:
  `models.pg.ts` / `models.mysql.ts` / `models.sqlite.ts` (`DrizzleDialect::ALL`)
- Constraint names go through the same `vespertide-naming` builders as the SQL
  layer (`build_unique_constraint_name` / `build_index_name` /
  `build_foreign_key_name`), so `drizzle-kit` sees the indexes vespertide created;
  SQLite FKs stay unnamed (SQLite stores no FK constraint names)
- FKs use the named `foreignKey({...})` operator in an array-form table callback;
  a self-referential key takes its foreign columns from the callback's `t`, which
  keeps the table const out of its own initializer's type inference
- The only backend that renders `TableConstraint::Check`, via its `check` builder with a `sql` template
- The `OrmExporter` trait path renders the Pg dialect (single-`String` trait);
  `render_schema(tables, dialect)` is the dialect-aware single-file entry point,
  so its snapshot tests live inline in the module (Prisma-exception pattern)

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
- Drizzle's cross-ORM snapshots carry the dialect the trait path renders (`…_Drizzle_pg.snap`); the other two dialects live in the module's own `render_schema_full_file_per_dialect@{pg,mysql,sqlite}` snapshots
- 646 snapshot files, all in the single shared `src/tests/snapshots/` directory; every export scenario goes through the shared `orm_cases!` macro in `src/tests/mod.rs`, producing one snapshot per ORM (all eight) — a scenario snapshotted for only one ORM is a defect

## NOTES

- YAML and JSON are both fully supported input formats; exporter tests also use YAML-formatted insta snapshots.
- Generated ORM files are outputs only; edit Vespertide models, then regenerate.
- Two-tier line policy (CI-enforced via `scripts/check-line-budget.sh`): production-only `.rs` ≤ 1000 lines; files carrying test code (`tests/` dir or inline `#[cfg(test)] mod tests`) ≤ 1200 lines.
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
