# Vespertide for Zed

> Language support for Vespertide schema files — by **[DevFive](https://devfive.kr)**.

Brings first-class editing for Vespertide JSON and YAML schemas to the [Zed editor](https://zed.dev) by wiring up the `vespertide-lsp` language server.

## Features

- **Diagnostics** — schema validation errors with precise byte ranges (unknown type, duplicate column, FK target missing, enum default invalid, filename ↔ table name mismatch, …).
- **Hover** — column type / FK target preview, with on-disk fallback for closed files.
- **Go to Definition** — F12 on `ref_table` or `ref_columns` entries jumps to the target table / column across files.
- **Find References** — Shift+F12 — all usages of a table or column workspace-wide.
- **Rename** — F2 with prepare-rename: column or table renames propagate to every `ref_columns` / `ref_table` in the workspace.
- **Completion** — context-aware suggestions for column types, ref_table, ref_columns, on_delete actions, `kind`, `default` (type-aware), and all 4 LSP key positions (table top-level, column object, foreign_key, type).
- **Code Actions** (`Ctrl+.`) — Mark as primary key, Convert to `varchar(N)` / `numeric(P,S)`, Extract default to enum, Add foreign_key skeleton, Toggle nullable, …
- **Document Symbol / Outline** — Ctrl+Shift+O — table → columns tree.
- **Workspace Symbol** — Ctrl+T — fuzzy search every table and column.
- **Inlay Hints** — column flags (PK · UQ · IX) and FK target (`⟶ user.id`) shown inline next to each `{`.
- **Semantic Tokens** — table / column / type / enum value coloured by *meaning*, not just syntax. See [Semantic colours](#semantic-colours) for theme setup.
- **Folding / Selection Range / Document Highlight** — standard LSP file-local features.
- **Drift Detection** — flags models that have diverged from the applied migration history. _Unique to Vespertide._

## Semantic colours

Zed's default themes don't always paint LSP semantic tokens out of the box. Add this to your `settings.json` (`Ctrl+,`) to get the DevFive brand palette — table names in violet, column names in teal, types in amber, enum values in pink:

```json
{
  "experimental.theme_overrides": {
    "syntax": {
      "type": { "color": "#f59e0b" },
      "type.builtin": { "color": "#f59e0b" }
    }
  }
}
```

If your theme already ships semantic styles (Solarized Dark, GitHub themes, etc.) you should see colours immediately — the log line `semantic_tokens_full ... tokens=N` in `$TEMP/vespertide-lsp.log` confirms the server is responding.

## Installation

### From the Zed extensions registry

Once published, open the command palette and run:

```
zed: extensions
```

Search for **Vespertide** and click _Install_.

### Local development (dev extension)

Clone this repository and from the Zed command palette run:

```
zed: install dev extension
```

Point the picker at `apps/zed-extension/`. The extension builds to WebAssembly and downloads the `vespertide-lsp` binary from the latest [GitHub Release](https://github.com/dev-five-git/vespertide/releases) on first use.

If a `vespertide-lsp` binary is already on your `PATH` (for example via `cargo install vespertide-cli` with the LSP feature, or a local debug build), the extension uses it directly — no download.

## Configuration

By default the extension matches files ending in `.vespertide`, `.vespertide.json`, `.vespertide.yaml`, and `.vespertide.yml`. To opt-in additional globs (for example the conventional `models/**/*.json` layout) add this to your Zed `settings.json`:

```json
{
  "file_types": {
    "Vespertide JSON": ["models/**/*.json"],
    "Vespertide YAML": ["models/**/*.yaml", "models/**/*.yml"]
  }
}
```

Per-project overrides go in `.zed/settings.json` at the repository root.

## Requirements

- Zed `0.155.0` or later (extension API `0.7`).
- One of:
  - A `vespertide-lsp` binary on `PATH`, **or**
  - Network access on first launch so the extension can pull the latest release asset from GitHub.

## License

Apache-2.0. See [LICENSE](./LICENSE).
