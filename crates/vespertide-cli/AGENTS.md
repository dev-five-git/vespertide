# vespertide-cli

CLI for declarative database schema management. Uses clap for argument parsing, colored output for user feedback.

## STRUCTURE

```
src/
├── main.rs           # Clap CLI definition, command dispatch
├── utils.rs          # Re-exports loader functions, migration filename generation
└── commands/
    ├── mod.rs        # Public exports: cmd_{init,new,diff,sql,revision,status,log,export}
    ├── init.rs       # Create vespertide.json
    ├── new.rs        # Create model template with $schema reference
    ├── diff.rs       # Show pending changes (colored action formatting)
    ├── sql.rs        # Print SQL for next migration
    ├── revision.rs   # Persist migration (interactive fill-with prompts)
    ├── status.rs     # Show config and sync status
    ├── log.rs        # List applied migrations with SQL
    └── export.rs     # Export to ORM code (SeaORM/SQLAlchemy/SQLModel)
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

## WHERE TO LOOK

| Task | File | Key Functions |
|------|------|---------------|
| Add new CLI command | `main.rs` | Add to `Commands` enum, match in `main()` |
| Modify action display | `diff.rs` | `format_action()`, `format_constraint_type()` |
| Change fill-with flow | `revision.rs` | `handle_missing_fill_with()`, `collect_fill_with_values()` |
| Export logic | `export.rs` | `walk_models()`, `ensure_mod_chain()`, `build_output_path()` |
| Filename patterns | `utils.rs` | `migration_filename_with_format_and_pattern()` |

## NOTES

- **revision.rs** (1100 lines): Most complex - handles interactive `--fill-with` prompts for NOT NULL columns without defaults
- **export.rs**: Generates `mod.rs` chain for SeaORM exports; Python ORMs skip this
- All commands use `load_config()`, `load_models()`, `load_migrations()` from `vespertide_loader`
- Tests use `serial_test::serial` with `CwdGuard` for directory isolation
- Schema URLs default to GitHub raw; override via `VESP_SCHEMA_BASE_URL` env var
