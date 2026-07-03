# Clippy `allow` Audit Report

**Generated:** 2026-05-24 (commit-time snapshot — original audit)
**Scope:** Every `#![allow(...)]` / `#[allow(...)]` / `#[expect(...)]` in `crates/`, `tools/`, `fuzz/`
**Trigger:** Owner questioning whether suppressions like `#![allow(dead_code)]` are truly necessary.

## ✅ STATUS UPDATE — migration complete

The audit's primary recommendation has been **fully executed**. Current verified state:

| Metric | Then (2026-05-24) | Now |
|---|---|---|
| `#[allow(...)]` / `#![allow(...)]` | 36 | **0** |
| `#[expect(...)]` (with mandatory `reason`) | 9 | **49** |

- **Every `allow` was migrated to `#[expect(...)]` or eliminated.** The workspace lint `allow_attributes` / `allow_attributes_without_reason` (warn) now keeps any new bare `allow` out.
- **No stale suppressions can survive.** `#[expect(lint)]` emits `unfulfilled_lint_expectations` if the lint stops firing; since `cargo clippy --workspace --all-targets --all-features -- -D warnings` is green, **all 49 expectations are currently fulfilled** (genuinely needed) by construction.
- The original 🔴/🟡/🟠 suspects were resolved, not masked (individually re-verified):
  - 🔴 The dead `cmd_erd` wrapper was **deleted** — only the genuinely-used `cmd_erd_with_filters` remains (called from `main.rs` and re-exported in `commands/mod.rs`). No `dead_code` suppression survives in `vespertide-cli`.
  - 🟠 `cache.rs`'s file-wide `#![allow(dead_code)]` was **removed entirely** (not merely narrowed) — every `RingCache` item is now genuinely reachable, proven by a green `clippy -D warnings`.
  - 🟡 The `visitors.rs` "dead-code" oracle functions are now `#[cfg(test)]`-gated (the canonical pattern documented in AGENTS.md), so they carry zero `dead_code` suppression.

The sections below are retained as the **original pre-migration snapshot** for historical context; the specific `#[allow]` file:line entries no longer exist as `#[allow]` (they are now `#[expect]` or removed).

---

## TL;DR (original audit, 2026-05-24)

Of **36 active `allow` attributes**, audit reveals:

| Status | Count | Action |
|---|---|---|
| 🟢 Genuinely justified | **27** | Migrate to `#[expect(...)]` (Rust 1.81+) so stale suppressions self-report |
| 🟡 Stale candidates (likely removable) | **7** | Try removing — if build stays clean, delete the suppression |
| 🔴 Hides actual dead code | **1** | Delete the unused item itself (`cmd_erd`) |
| 🟠 Overly broad (file-level when item-level would suffice) | **1** | Narrow to specific items in `cache.rs` |

**Verdict:** ~22% (8/36) of current `allow` attributes are suspect. The owner's instinct is correct — these patterns *do* warrant scrutiny, but most surviving suppressions are defensible. The rest is a one-off cleanup, not a systemic problem.

---

## 1. Inventory (raw)

### 1.1 `#![allow(...)]` — inner attributes (file/module-wide) — 14

| File | Lint | Reason |
|---|---|---|
| `vespertide-cli/src/commands/erd/svg.rs:18` | 9 cast/wrap/format lints | SVG coordinate math — file-wide cast/precision suppressions are pragmatic for renderer code |
| `vespertide-lsp/src/backend/handler_file_features.rs:6` | `clippy::unused_async`, `deprecated` | LSP trait requires `async fn` even when body has no `await`; deprecated LSP capability kept for client compat |
| `vespertide-lsp/src/backend/handler_rename.rs:11` | `clippy::unused_async` | Same — `tower-lsp-server` trait shape |
| `vespertide-lsp/src/backend/helpers.rs:7` | `deprecated` | Bridges deprecated LSP `SymbolInformation` for downlevel clients |
| `vespertide-lsp/src/cache.rs:2` | `dead_code` | **🟠 Too broad** — see §3.3 |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:1` | `dead_code` | **🟡 Stale candidate** — see §3.1 |
| `vespertide-lsp/src/semantic_tokens/classify_json.rs:25` | `clippy::struct_excessive_bools` | Token classifier holds flag set; collapsing to enum would obscure intent |
| `vespertide-lsp/src/semantic_tokens/classify_yaml.rs:20` | `clippy::struct_excessive_bools` | Same |
| `vespertide-lsp/src/semantic_tokens/encode.rs:6` | `clippy::cast_possible_truncation` | LSP `semanticTokens` wire format mandates `u32` deltas; values are bounded by file size |
| `vespertide-planner/src/diff/tests/mod.rs:1` | `clippy::module_inception` | Conventional `mod tests { mod tests; ... }` pattern |
| `vespertide-planner/tests/diff_parallel.rs:1` | `unsafe_code` | `serial_test` env-var manipulation requires `unsafe` blocks |
| `vespertide-planner/tests/validate_parallel.rs:1` | `unsafe_code` | Same |
| `vespertide-planner/tests/validate_schema_parallel.rs:1` | `unsafe_code` | Same |
| `vespertide-query/tests/parallel_build.rs:1` | `unsafe_code` | Same |

### 1.2 `#[allow(...)]` — outer attributes (item-level) — 22

| File:Line | Lint | Item | Verdict |
|---|---|---|---|
| `vespertide-cli/src/commands/erd/mod.rs:26` | `dead_code` | `cmd_erd` async fn | **🔴 Real dead code** — see §3.2 |
| `vespertide-lsp/src/backend/mod.rs:113` | `deprecated` | `symbol_to_lsp` | LSP back-compat — justified |
| `vespertide-lsp/src/backend/mod.rs:296` | `deprecated` | `initialize_root_uri` | LSP `rootUri` is officially deprecated in favor of `workspaceFolders`; still required for older clients |
| `vespertide-lsp/src/backend/mod.rs:743` | `deprecated` | `symbol` async handler | Same — `workspace/symbol` legacy shape |
| `vespertide-lsp/src/diagnostics/validation/mod.rs:14` | `unused_imports` | `pub(super) use visitors::{...}` | Façade re-export of items used conditionally elsewhere — investigate; possibly stale |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:110` | `dead_code` | `collect_syntax_errors` | **🟡 Stale candidate** — function has 3 usages |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:125` | `dead_code` | `collect_unknown_column_types` | **🟡 Stale candidate** — 3 usages |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:139` | `dead_code` | `walk_column_objects` | **🟡 Stale candidate** — 3 usages |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:214` | `dead_code` | `collect_duplicate_column_names` | **🟡 Stale candidate** — 3 usages |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:309` | `dead_code` | `collect_complex_type_violations` | **🟡 Stale candidate** — 3 usages |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:322` | `dead_code` | `walk_columns_for_complex_type` | **🟡 Stale candidate** — 3 usages |
| `vespertide-lsp/src/diagnostics/validation/visitors.rs:561` | `dead_code` | `walk_for_errors` | **🟡 Stale candidate** — 3 usages |
| `vespertide-lsp/src/diagnostics/validation/types.rs:147` | `dead_code` | `find_value_for_key` | 13 usages — definitely stale |
| `vespertide-lsp/src/rename.rs:46` | `clippy::too_many_arguments` | `compute(...)` | LSP rename collects (uri, position, params, doc, kind, store, references, ...) — justified; consider `RenameContext` struct in 0.3.x |
| `vespertide-lsp/src/references/search.rs:13` | `clippy::too_many_arguments` | `find_all(...)` | Same pattern — justified |
| `vespertide-lsp/src/references/mod.rs:151` | `clippy::too_many_arguments` | `compute(...)` | Same |
| `vespertide-lsp/src/position.rs:23` | `clippy::cast_possible_truncation` | `byte_to_lsp_position` | UTF-16 column math bounded by LSP spec; justified |
| `vespertide-macro/src/lib.rs:92` | `clippy::unnecessary_wraps` | `vespertide_crate_path` | proc-macro convention: helpers return `syn::Result<TokenStream2>` for uniform call sites |
| `vespertide-query/src/sql/add_constraint/foreign_key.rs:8` | `clippy::too_many_arguments` | `build_foreign_key` | Composite FK builder — justified, narrow generics; refactor candidate |
| `vespertide-query/src/sql/modify_column_nullable.rs:15` | `clippy::too_many_arguments` | `build_modify_column_nullable` | Same pattern |
| `vespertide-query/src/error.rs:73` | `deprecated` | `other_variant_still_constructable_for_backward_compat` | **G.3 0.2.0 work** — explicit test that deprecated variant still constructs; ideal `#[expect(...)]` candidate |
| `vespertide-query/tests/table_prefixed_enum_test.rs:10` | `clippy::too_many_lines` | `test_table_prefixed_enum_names` | Single integration test; split into focused cases recommended but not urgent |

### 1.3 `#[expect(...)]` — modern alternative — 9

Already in use in 5 files. **This is the migration target for the justified `allow`s above.**

---

## 2. Why `#![allow(dead_code)]`-style suppressions are dangerous

`#[allow(X)]` is a **silent contract**: "I know X applies here, accept it."

The contract has two failure modes:

1. **The hidden defect lives forever.** A `#[allow(dead_code)]` placed on a function during refactoring keeps the suppression after the function becomes actually-used again. The next dead version of the function is invisible.
2. **The suppression outlives the underlying lint trigger.** Code evolves; the violation goes away. The `allow` remains, masking nothing — pure noise, often misleading future readers ("why is this allowed?").

These failure modes are precisely what the owner intuits with the question.

### The fix: `#[expect(...)]` (stabilized in Rust 1.81)

```rust
// Old — silent, perpetual
#[allow(dead_code)]
fn foo() {}

// New — self-reporting
#[expect(dead_code, reason = "Reserved for HS-12 follow-up; tracked in #N")]
fn foo() {}
```

If `foo` becomes used, the lint stops firing → compiler emits `unfulfilled_lint_expectations` → the stale `#[expect]` is flagged.

This crate already uses `#[expect(...)]` in 9 places. **The audit's primary recommendation is to migrate every justified `#[allow]` to `#[expect]`.**

---

## 3. Suspect cases in detail

### 3.1 `vespertide-lsp/src/diagnostics/validation/visitors.rs` — 1 inner + 7 outer `dead_code` (🟡 likely all stale)

**Evidence:**
- All 8 suppressed functions have ≥3 usages reported by `grep`.
- The pattern matches a Wave A3 + A4 split (per `AGENTS.md`) where individual `collect_*` callers were fused into `collect_all`, but the per-function entry points were retained for testing without re-checking which `#[allow]` annotations were still warranted.
- File-level `#![allow(dead_code)]` is layered on top of 7 item-level `#[allow(dead_code)]` — **double suppression** is a strong indicator that the file was edited under deadline pressure and never re-audited.

**Recommended action (in this order):**
1. Remove the file-level `#![allow(dead_code)]` on line 1.
2. `cargo build -p vespertide-lsp 2>&1 | grep dead_code` — observe which item-level allows are still necessary.
3. Remove every item-level `#[allow(dead_code)]` that no longer triggers a warning.
4. For the remaining (genuinely dead) items, decide: delete them, or migrate to `#[expect(dead_code, reason = "...")]` with a tracking comment.

**Risk:** Low — purely diagnostic; reverting is trivial.

### 3.2 `vespertide-cli/src/commands/erd/mod.rs:26` — `cmd_erd` (🔴 actually dead)

```rust
#[allow(dead_code)]
pub async fn cmd_erd(format: ErdFormat, output: Option<PathBuf>) -> Result<()> {
    cmd_erd_with_filters(format, output, Vec::new(), Vec::new(), 0).await
}
```

**Evidence:**
- Function delegates straight to `cmd_erd_with_filters`.
- All call sites (`lib.rs`, match arm) use `cmd_erd_with_filters` directly.
- No external `pub use cmd_erd;` re-export.

**Recommended action:** **Delete the function and its `#[allow(dead_code)]`.** No callers; the `_with_filters` variant already accepts empty `Vec::new()` slots when the user invokes the no-filter path.

### 3.3 `vespertide-lsp/src/cache.rs:2` — file-level `#![allow(dead_code)]` (🟠 too broad)

**Evidence:**
- `RingCache` — 8 usages outside the file
- `docstore_fingerprint` — 6 usages outside the file
- `hash_text` — 14 usages outside the file

The file's public surface is heavily used. The `dead_code` warning likely fires for a single specific helper (probably `RingCache::clear` outside `#[cfg(test)]`, or an unused inner getter).

**Recommended action:**
1. Remove the file-level `#![allow(dead_code)]`.
2. `cargo build -p vespertide-lsp 2>&1 | grep dead_code` — identify the actual offender.
3. Apply a narrow `#[expect(dead_code, reason = "...")]` to the *specific item*, or remove the item if it's truly unused.

---

## 4. Justified suppressions worth preserving

These 27 entries pass scrutiny but should still migrate to `#[expect(...)]`:

| Category | Count | Justification |
|---|---|---|
| `unsafe_code` in `tests/*_parallel.rs` | 4 | `serial_test` env-var manipulation; AGENTS.md mandates `unsafe_code = "warn"` workspace-wide |
| `deprecated` for LSP back-compat | 6 | `tower-lsp-server` & `lsp-types` mark `rootUri`, deprecated `SymbolInformation`, etc. — we still support clients that send them |
| `clippy::unused_async` on LSP trait methods | 2 | Trait shape forces `async fn` even when body is sync |
| `clippy::struct_excessive_bools` on classifiers | 2 | Semantic-token flag sets — collapsing obscures intent |
| `clippy::cast_possible_truncation` for wire formats | 2 | LSP/`tree-sitter` mandate `u32`; values bounded by file size |
| `clippy::too_many_arguments` on builders | 5 | LSP rename / FK builder / references — 0.3.x refactor candidates (extract `*Context` struct) |
| `clippy::module_inception` in `diff/tests/mod.rs` | 1 | Conventional Rust test module pattern |
| `clippy::unnecessary_wraps` on proc-macro helper | 1 | `syn::Result<TokenStream2>` uniform signature |
| `clippy::too_many_lines` on integration test | 1 | Single long test; split recommended but not urgent |
| `unused_imports` on façade re-export | 1 | Investigate first — might be stale |
| Inline `#![allow(...)]` in `erd/svg.rs` (9 cast lints) | 1 file | SVG coordinate math; file-wide is pragmatic here |
| Backward-compat test for `QueryError::Other` | 1 | **G.3 0.2.0 explicit deprecation test** — ideal `#[expect]` showcase |

---

## 5. Recommended cleanup plan (effort estimate)

| Step | Effort | Risk | Value |
|---|---|---|---|
| 1. Delete `cmd_erd` (§3.2) | 5 min | None | Removes 1 false-justified suppression |
| 2. Remove `visitors.rs` allows, rebuild, prune (§3.1) | 15 min | Low | Removes ~8 stale suppressions |
| 3. Narrow `cache.rs` allow (§3.3) | 10 min | Low | Restores dead-code detection for the rest of the file |
| 4. Migrate 27 justified `allow` → `expect` with reason strings | 30 min | None | Future suppressions self-report when stale |
| 5. Add `#![warn(clippy::allow_attributes_without_reason)]` to crate roots | 5 min | None | Prevents undocumented future `allow` |
| **Total** | **~65 min** | **Low** | **High** — eliminates ~8 suspect suppressions, prevents future regressions |

---

## 6. Why this audit matters (philosophical)

`#[allow]` is one of three ways to opt out of compiler/clippy guidance:

1. **`#[allow(X)]`** — silent, permanent
2. **`#[expect(X)]`** — explicit, self-cleaning
3. **`#[deny(X)] → fix the code`** — actually solve it

Default to #3. When the lint is wrong for *this specific code* (LSP trait shape, wire format, idiom), use #2 with a reason. Never reach for #1 without acknowledging that you are creating a future maintenance burden.

The owner's question — "is this really necessary?" — is the right question to ask of every `#[allow]`. This audit shows that for this codebase, the answer is **mostly yes, but ~22% of cases warrant cleanup**, and the migration to `#[expect]` would make the question answerable mechanically next time instead of requiring a manual audit.

---

## 7. Reproduce this audit

```powershell
# Count current allows
Get-ChildItem -Recurse -Include "*.rs" crates, tools, fuzz |
    ForEach-Object { Select-String -Path $_.FullName -Pattern "^\s*#!?\[allow\(" } |
    Measure-Object | Select-Object -ExpandProperty Count

# Detect stale (after migration to #[expect])
cargo clippy --workspace --all-targets --all-features -- -D warnings -W unfulfilled_lint_expectations
```
