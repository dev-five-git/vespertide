# vespertide-cli

CLI for declarative database schema management. Uses clap for argument parsing, colored output for user feedback.

## STRUCTURE

```
src/
├── main.rs           # Clap CLI definition, command dispatch
├── utils.rs          # Re-exports loader functions, migration filename generation
└── commands/
    ├── mod.rs        # Public exports: cmd_{init,new,diff,sql,revision,status,log,export} + cmd_erd_with_filters
    ├── init.rs       # Create vespertide.json
    ├── new.rs        # Create model template with $schema reference
    ├── diff/         # Show pending changes (colored action formatting) — mod.rs + tests/
    ├── sql.rs        # Print SQL for next migration
    ├── revision/     # Persist migration — mod.rs, parse.rs, emit.rs, write.rs, timezones.rs,
    │                 #   prompts/ (fill_with, narrowing, timezone, drop_recreate_fk_policy,
    │                 #   choices_and_apply/), tests/
    ├── status.rs     # Show config and sync status
    ├── log.rs        # List applied migrations with SQL
    ├── export/       # Export to ORM code (SeaORM/SQLAlchemy/SQLModel/JPA/Prisma/Drizzle) —
    │                 #   mod.rs + tests/ (mod.rs, prisma.rs, drizzle.rs)
    └── erd/          # ERD diagram export — mod.rs, mermaid.rs, dot.rs, svg/ (style, model,
                      #   layout, edges, render, util), tests/
```

## COMMANDS

| Command | Function | Key Logic |
|---------|----------|-----------|
| `init` | `cmd_init()` | Writes default `VespertideConfig` as JSON |
| `new <name>` | `cmd_new(name, format)` | Template with `$schema` URL for IDE support |
| `diff` | `cmd_diff()` | `plan_next_migration()` + colored `format_action()` |
| `sql` | `cmd_sql(backend)` | `build_action_queries()` + `query.build(backend)` |
| `revision -m` | `cmd_revision(msg, fill_with)` | Interactive prompts via `dialoguer::Input` |
| `status` | `cmd_status()` | Display config paths and migration count |
| `log` | `cmd_log(backend)` | Iterate applied migrations, print SQL |
| `export --orm` | `cmd_export(orm, dir)` | `render_entity_with_schema()` + mod.rs wiring |
| `erd -f svg\|mermaid\|dot` | `cmd_erd_with_filters(format, output, include, exclude, depth)` | FK-graph filtered ERD rendering |

## WHERE TO LOOK

| Task | File | Key Functions |
|------|------|---------------|
| Add new CLI command | `main.rs` | Add to `Commands` enum, match in `main()` |
| Modify action display | `diff/mod.rs` | `format_action()`, `format_constraint_type()` |
| Change fill-with flow | `revision/prompts/fill_with.rs` | fill-with prompt + collection helpers |
| Export logic | `export/mod.rs` | `walk_models()`, `ensure_mod_chain()`, `build_output_path()` |
| ERD rendering | `erd/` | `cmd_erd_with_filters()`, `svg/render.rs`, `mermaid.rs`, `dot.rs` |
| Filename patterns | `utils.rs` | `migration_filename_with_format_and_pattern()` |

## NOTES

- **revision/**: Most complex command — handles interactive `--fill-with` prompts for NOT NULL columns without defaults; long ago split from a single 3064-line file into `revision/{mod,parse,emit,write,timezones}.rs` + `prompts/` + `tests/`
- **export/**: Generates the `mod.rs` chain for SeaORM exports; Python/Java ORMs skip it. Prisma and Drizzle take separate single-file paths rather than one file per model — Prisma writes one `models.prisma`, Drizzle one file per dialect (`models.pg.ts` / `models.mysql.ts` / `models.sqlite.ts`)
- All commands use `load_config()`, `load_models()`, `load_migrations()` from `vespertide_loader`
- YAML and JSON are both fully supported for models and migrations; `new <name> -f yaml` creates YAML templates.
- Prefer typed `MigrationAction` enums; `RawSql` exists as a documented emergency escape hatch, but is not recommended for normal use.
- Tests use `serial_test::serial` with `CwdGuard` for directory isolation
- Schema URLs default to GitHub raw; override via `VESP_SCHEMA_BASE_URL` env var
- Two-tier line policy (CI-enforced via `scripts/check-line-budget.sh`): production-only `.rs` ≤ 1000 lines; files carrying test code (`tests/` dir or inline `#[cfg(test)] mod tests`) ≤ 1200 lines.
- Workspace lints warn on unsafe code and Clippy all: `unsafe_code = "warn"`, `clippy::all = { level = "warn", priority = -1 }`.
