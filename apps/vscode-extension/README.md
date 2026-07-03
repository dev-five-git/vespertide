# Vespertide for Visual Studio Code

> Declarative database schema management — directly in your editor.

Vespertide brings first-class language support for Vespertide schema files to VS Code: rich diagnostics, hover hints, go-to-definition, smart completion, and — uniquely — **live drift detection** between your declared models and applied migration history.

<p align="center">
  <strong style="color:#5b34f7">Built by <a href="https://github.com/dev-five-git">DevFive</a></strong>
</p>

---

## Features

- **Diagnostics** — Real-time validation of `models/*.json` and `models/*.yaml` files. Catch invalid column types, broken foreign keys, and ENUM mismatches before you ever run `vespertide diff`.
- **Hover** — Inspect column types, constraints, and ENUM members without leaving your schema file.
- **Go-to-Definition** — Jump from foreign-key references straight to the target table.
- **Completion** — Context-aware suggestions for column types, table names, and constraint kinds.
- **🟣 Drift Detection** — **The killer feature no other schema tool offers.** Vespertide continuously compares your declared models against your migration history and surfaces drift inline — the exact lines that diverge, highlighted as you type.

---

## Installation

Install **Vespertide** from the [Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=dev-five-git.vespertide) or the [Open VSX Registry](https://open-vsx.org/extension/dev-five-git/vespertide).

```bash
code --install-extension dev-five-git.vespertide
```

The extension ships with the `vespertide-lsp` binary for your platform — no separate install required.

---

## Usage

Open any project that contains a `models/` directory with `.json`, `.yaml`, or `.yml` schema files, or a `vespertide.json` config at the workspace root.

The language server activates automatically and starts publishing diagnostics. The Vespertide status bar item (bottom-left) shows the connection state.

---

## Configuration

| Setting | Default | Description |
| --- | --- | --- |
| `vespertide.serverPath` | `""` | Override path to the `vespertide-lsp` binary. Leave empty to use the bundled binary. |
| `vespertide.logLevel` | `"info"` | Server log level (`off`, `error`, `warn`, `info`, `debug`, `trace`). |
| `vespertide.trace.server` | `"off"` | Trace LSP protocol messages between editor and server. Useful for debugging. |

---

## Commands

| Command | ID |
| --- | --- |
| Vespertide: Restart Language Server | `vespertide.restartServer` |

---

## Requirements

- **Visual Studio Code** `1.105.0` or newer

---

## License

[Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) © DevFive
