# vespertide

Rust workspace for defining database schemas in JSON (YAML planned) and generating migration plans and SQL from model diffs. Ships with a CLI and JSON Schemas for validation.

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![GitHub Actions](https://img.shields.io/github/actions/workflow/status/dev-five-git/vespertide/CI.yml?branch=main&label=CI)](https://github.com/dev-five-git/vespertide/actions)
[![Codecov](https://img.shields.io/codecov/c/github/dev-five-git/vespertide)](https://codecov.io/gh/dev-five-git/vespertide)
[![GitHub stars](https://img.shields.io/github/stars/dev-five-git/vespertide.svg?style=social&label=Star)](https://github.com/dev-five-git/vespertide)
[![GitHub forks](https://img.shields.io/github/forks/dev-five-git/vespertide.svg?style=social&label=Fork)](https://github.com/dev-five-git/vespertide/fork)
[![GitHub issues](https://img.shields.io/github/issues/dev-five-git/vespertide.svg)](https://github.com/dev-five-git/vespertide/issues)
[![GitHub pull requests](https://img.shields.io/github/issues-pr/dev-five-git/vespertide.svg)](https://github.com/dev-five-git/vespertide/pulls)
[![GitHub last commit](https://img.shields.io/github/last-commit/dev-five-git/vespertide.svg)](https://github.com/dev-five-git/vespertide/commits/main)
[![OpenAPI](https://img.shields.io/badge/OpenAPI-3.1-green.svg)](https://www.openapis.org/)


## Components
- `crates/vespertide-core`: Data models for tables, columns, constraints, indexes, and migration actions.
- `crates/vespertide-planner`: Replays applied migrations to rebuild a baseline, then diffs against current models to compute the next migration plan.
- `crates/vespertide-query`: Converts migration actions into PostgreSQL SQL statements with bind parameters.
- `crates/vespertide-config`: Manages models/migrations directories and naming-case preferences.
- `crates/vespertide-cli`: `vespertide` command (model template, diff, SQL, revision, status, log).
- `crates/vespertide-schema-gen`: Emits JSON Schemas (`schemas/`).
- `crates/vespertide-macro`: Macro entry for runtime migration execution (logic not implemented yet).
- `examples/app`: Minimal sample project (`vespertide.json`, `models/user.json`).

## Quickstart
1) Build the workspace
```
cargo build
```
2) Create default config
```
cargo run -p vespertide-cli -- init
```
3) Create a model template (e.g., `user.json`)
```
cargo run -p vespertide-cli -- new user
```
4) Edit the model and generate a migration
```
cargo run -p vespertide-cli -- revision -m "initial schema"
```
5) Preview the planned SQL
```
cargo run -p vespertide-cli -- sql
```

## CLI Commands
- `init`: Create `vespertide.json`.
- `new <name> [-f json|yaml|yml]`: Create a model template with `$schema` (loader currently supports JSON only).
- `diff`: Summarize changes between applied migrations and current models.
- `sql`: Print SQL for the next migration plan (with binds).
- `revision -m "<msg>"`: Persist pending changes as a migration JSON file.
- `status`: Show config/models/migrations overview and sync status.
- `log`: List applied migrations and generated SQL in chronological order.

## Model & Migration Format
- Model (`TableDef`): Table name, columns, table constraints (Primary/Unique/Foreign/Check), indexes in JSON.
- Migration (`MigrationPlan`): Version, comment, and action list—create/delete/rename table, add/delete/rename/modify column, add/remove index.
- JSON Schemas live in `schemas/`; override the base URL with `VESP_SCHEMA_BASE_URL`.

## Limitations & Roadmap
- Runtime executor (`run_migrations` in `vespertide-macro`) is not implemented.
- YAML loading is not supported; only JSON is parsed (YAML templates can be generated but not read).
- SQL generation targets PostgreSQL.

## Development
- Format/lint/test: `cargo fmt`, `cargo clippy --all-targets --all-features`, `cargo test`.
- Regenerate schemas: `cargo run -p vespertide-schema-gen -- --out schemas`.

## Example
- In `examples/app`, run `cargo run -p vespertide-cli -- diff` to see it in action.