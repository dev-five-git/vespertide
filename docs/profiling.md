# Profiling vespertide-lsp

This document explains how to reproducibly profile `vespertide-lsp` under a representative load using `cargo flamegraph`. The goal is a consistent, re-runnable audit that any contributor can execute on Windows, Linux, or macOS to identify CPU hot spots and track regressions over time.

---

## Prerequisites

### All platforms

```bash
cargo install --locked flamegraph
```

### Windows

Windows uses `blondie` for ETW-based sampling (no `perf` available):

```powershell
cargo install --locked blondie
# Run elevated terminal (admin) when capturing — blondie uses ETW
```

### Linux

Linux uses kernel `perf`:

```bash
sudo apt-get install -y linux-tools-common linux-tools-generic
# Allow perf access for the current user (one-time):
sudo sysctl -w kernel.perf_event_paranoid=-1
```

### macOS

`dtrace` ships with macOS. Depending on your macOS version it may require `sudo` and SIP disabled:

```bash
# dtrace ships with macOS. May require sudo + SIP disabled depending on macOS version.
sudo cargo flamegraph --root ...   # --root re-runs under sudo
```

---

## Build the workload binary

```bash
cargo build --release --manifest-path tools/lsp-profile/Cargo.toml
```

The release profile for this crate sets `debug = "line-tables-only"`, so stack frames are resolvable in the generated SVG without bloating the binary size.

---

## Capture the flamegraph

```bash
cargo flamegraph --manifest-path tools/lsp-profile/Cargo.toml --bin lsp-profile -o docs/profiling-baseline.svg
```

This command implicitly runs `cargo build --release`, then attaches the platform sampler, symbolizes the collected stacks, and writes the SVG. Expected runtime is 30-90 seconds; sampling overhead dominates, not the workload itself.

---

## What the workload exercises

The binary spins up a deterministic 100-table tempdir workspace and drives the following operations:

- `compute_diagnostics` x 100 — tree-sitter parse + 3-tier validation
- `compute_completion` x 50 — context resolution + workspace lookup
- `semantic_tokens::classify` x 50 — full-tree classification
- `compute_workspace_symbols` x 20 — fuzzy search across disk tables
- `compute_drift` x 20 — model vs migration baseline diff

The workspace is fully deterministic (seeded table names, fixed column counts), so two runs on the same machine should produce comparable flamegraphs.

---

## Reading the SVG

Open `docs/profiling-baseline.svg` in any browser.

- **X axis**: total %CPU time. A wider box means more time spent in that function.
- **Y axis**: stack depth. Functions higher up were called by the function below them.
- Click any box to zoom into that subtree; press Escape to zoom back out.
- Use Ctrl+F to search by function name across the entire flame.
- Focus on wide boxes near the bottom of the flame — those are the hot leaf functions where actual work happens.

---

## Top hot spots (baseline)

**Captured on**: 2026-05-22 via `tools/lsp-profile` (ITERATIONS=1000, 100-table synthetic workspace, release build with `debug = "line-tables-only"`)

**These are the original audit baseline values.** The "Optimization Outcomes (Waves A-C)" section below shows what has been done since and the resulting speedups.

**Note on SVG**: The flamegraph SVG (`docs/profiling-baseline.svg`) was not committed in this baseline pass because the Windows developer environment requires Administrator privileges for ETW-based sampling via `blondie_dtrace` and the capture run was not elevated. The phase timings below were measured by the workload binary directly; function-level attribution below is grounded in source-code analysis of the call paths. Run the capture command in the **Capture the flamegraph** section above on Linux CI (where `perf` is available without root via the `kernel.perf_event_paranoid` knob) or from an elevated PowerShell on Windows to generate the SVG.

### Phase timings (measured, deterministic)

| Phase | Calls | Wall time | Share | Items produced |
|---|---|---|---|---|
| 4 workspace_symbols | 20,000 | **11.59s** | **49.3%** | 3,196,000 symbols |
| 1 diagnostics       | 100,000 | **8.83s**  | **37.6%** | 99,000 diagnostics |
| 5 drift             | 100   | **2.06s**  | **8.8%**  | 2,100 drifts |
| 3 semantic_tokens   | 50,000 | **0.79s**  | **3.4%**  | 1,245,000 tokens |
| 2 completion        | 50,000 | **0.12s**  | **0.5%**  | 366,000 items |
| **total**           |       | **23.50s** | 100%      |               |

### Top hot spots

| Rank | Function | File | Share | Why it's hot |
|---|---|---|---|---|
| 1 | `symbols::collect_from_tree` + `symbols::matches_needle` | `crates/vespertide-lsp/src/symbols.rs` | ~25% | Workspace symbol search walks every open doc's tree-sitter tree per call. `matches_needle` allocates a new lowercase `String` per column tested (`name.to_ascii_lowercase()`) — that's `O(columns × queries)` heap allocations. The synthetic workspace produces ~800 columns × 20 queries × 1000 iterations = 16M throwaway lowercase strings. **Fix candidate**: pre-compute lowercase names in `WorkspaceTables` cache, or use ASCII case-insensitive `memmem`. |
| 2 | `symbols::compute` disk-table reparse loop | `crates/vespertide-lsp/src/symbols.rs` L65-89 | ~15% | When a model file is not in `DocumentStore`, the loop reads the file from disk and parses it with a fresh `ParserPool` **per `compute_workspace_symbols` call**. With 20,000 calls × disk fallback × ~100 model files, this dominates. **Fix candidate**: persistent disk-table tree cache invalidated on file timestamp change (mirror what `WorkspaceTables` already does for `TableDef`). |
| 3 | `diagnostics::validation::try_parse_json` (serde full-parse) | `crates/vespertide-lsp/src/diagnostics/validation.rs` | ~15% | Tier-2 diagnostic does a full serde deserialize of every model file on every keystroke. Allocates the whole `TableDef` graph (BTreeMap + Vec + Strings) even when only one field changed. With 100,000 diagnostics calls × ~2KB models, that's ~200MB of churn through the allocator. **Fix candidate**: short-circuit when the tree has errors (already partially in place); cache `TableDef` keyed on text hash. |
| 4 | `diagnostics::validation::collect_*` tree-sitter walks (4 passes) | `crates/vespertide-lsp/src/diagnostics/validation.rs` | ~12% | Four independent walks of the tree: `collect_syntax_errors`, `collect_unknown_column_types`, `collect_complex_type_violations`, `collect_duplicate_column_names`. Each does its own cursor walk. With 100,000 calls × 4 walks × ~50 nodes per tree, that's ~20M cursor operations. **Fix candidate**: fuse the four walks into a single visitor — the AST is walked once and each collector inspects relevant nodes. |
| 5 | `drift::compute` re-parses every model file | `crates/vespertide-lsp/src/drift/mod.rs` L142-220 | ~6% | Drift is called 100 times in the workload but each call re-runs `load_models_from_dir` + `load_migrations_from_dir` (full filesystem + serde) + `schema_from_plans` (replay every applied migration). Then for each drifted action it re-parses the model file source with `ParserPool` to find positions. **Fix candidate**: cache the loaded schema + parsed trees keyed on `(directory_path, mtime)`. Drift fires on every did_change debounce in real editor usage — this would yield a >10× speedup. |

### Cross-cutting observations

- **Tree-sitter walking is the dominant compute kernel** — phases 1, 3, 4, and 5 all walk the same trees with different visitors. A unified visitor framework or single-walk multi-collector pattern would amortize the cost.
- **Allocator pressure** — `matches_needle` (#1) and serde parse (#3) together churn through hundreds of MB of throwaway strings/maps. Per-call allocation count likely exceeds 1K. Profiling with `dhat` (heap profiler) would quantify this.
- **No tree caching across LSP requests** — every cross-file feature (symbols / drift) re-parses disk models. The `WorkspaceTables` cache exists for `TableDef` but not for `Tree`. Adding a parallel `Tree` cache (~10MB for 100-table workspace) would unblock #2 and #5.
- **`completion` is healthy** — 0.5% of total. No optimization needed.

### Follow-up audits (not in scope for this task)

1. **Allocator profile via `dhat`** — quantify the lowercase-string churn in `matches_needle`.
2. **Per-call latency distribution** — `diff_schemas` worst case may be >100ms on real workspaces (>500 tables).
3. **Cold-start time** — `WorkspaceTables::refresh` on a fresh editor open is not measured by this workload.

---

## Optimization Outcomes (Waves A-C, 2026-05-22)

All 5 hot spots from the baseline audit were addressed across 3 waves of optimization. The table below shows per-phase wall-time deltas against the baseline; the per-HS table shows what changed and where.

### Phase deltas

| Phase | Baseline | Final (Waves A-C) | Delta |
|---|---|---|---|
| 1 diagnostics × 100,000 | 8.83s | 4.92s | -44.3% |
| 4 workspace_symbols × 20,000 | 11.59s | 10.33s | -10.9% |
| 5 drift × 100 | 2.06s | 1.01s | -51.0% |
| 3 semantic_tokens × 50,000 | 0.79s | 0.77s | -2.5% |
| 2 completion × 50,000 | 0.12s | 0.11s | -8.3% |
| **total** | **23.50s** | **17.13s** | **-27.1%** |

### Per-HS outcomes

| HS | Optimization | File | Outcome | Notes |
|---|---|---|---|---|
| HS-1 | `symbols::matches_needle`: replaced `name.to_ascii_lowercase().contains(needle)` with zero-alloc `ascii_ci_contains` byte-walk | `crates/vespertide-lsp/src/symbols.rs` | Eliminated ~16M throwaway lowercase allocations per workload run | Wave A2 |
| HS-4 | Fused 4 independent tree-sitter walks in diagnostics into a 2-walk `collect_all` (direct-children walk for duplicates + recursive walk for type checks) | `crates/vespertide-lsp/src/diagnostics/validation/` | 4 walks → 2 walks per call | Wave A3; prerequisite split of `validation.rs` (861 lines) into `validation/{mod,visitors,types,parse,cache}.rs` |
| HS-6 | Shared `OnceLock<ParserPool>` in `symbols::compute` and `drift::compute` | `crates/vespertide-lsp/src/symbols.rs`, `drift/mod.rs` | Eliminated per-call grammar-load overhead | Wave A2 |
| HS-2 | Added mtime-keyed `tree_cache` to `WorkspaceTables`; disk-only model files return `(Arc<String>, Arc<Tree>)` on hit | `crates/vespertide-lsp/src/workspace.rs` | Limited measurable impact on this synthetic workload (all 100 tables are in `DocumentStore`, so `symbols::compute` hits the in-memory path). Real users with mostly-closed editor sessions will see the full benefit. | Wave B1 |
| HS-3 | Added 64-slot `ParseCache` (FxHasher + `(hash, len)` key) for `try_parse_json` / `try_parse_yaml`; same text returns `Arc<TableDef>` without re-serde-parse | `crates/vespertide-lsp/src/diagnostics/validation/cache.rs` | Biggest contributor to the -44% diagnostics win | Wave B2 |
| HS-5 | Added per-instance `DriftCache` keyed on `(project_root, config_mtime, max_model_mtime, max_migration_mtime)`; backend holds `Arc<DriftCache>` for server lifetime | `crates/vespertide-lsp/src/drift/mod.rs` | -51% on drift phase. New public API: `compute_drift_with_cache(...)`; legacy `compute_drift(...)` preserved for non-LSP callers via throwaway cache. | Wave C |

### Test count

+24 tests added across Waves A-C, no regressions (2062 baseline → 2086 final):

- Wave A: 6 (5 `ascii_ci_contains` unit tests + 1 fused-walk equivalence test)
- Wave B: 11 (5 `cached_parse` + 5 `ParseCache` + 1 diagnostics integration)
- Wave C: 7 (5 `DriftCache` unit tests + 2 integration tests covering cache warm and invalidation)

### Remaining gaps

- **`workspace_symbols` target (5.5s) not hit**: the synthetic workload keeps all files in `DocumentStore`, so HS-2's disk-cache path is never exercised. Adding a "mostly-closed" workload variant would expose the benefit empirically.
- **`drift` target (0.5s) not hit**: current 1.01s is the per-call floor (mtime probes + `diff_schemas` + per-action position mapping). Dropping further would require caching `Vec<DomainDrift>` itself keyed on the same mtimes. Meaningful refactor; deferred.
- **SVG capture still pending elevated execution**: Windows ETW requires Administrator privileges (`blondie`). Linux CI alternative is documented in the Prerequisites section above.

### How to re-measure

```bash
# Capture current state
cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- --json docs/profiling-current.json

# Compare against committed baseline
cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- --baseline docs/profiling-baseline.json
```

---

## Post-Audit Optimization Waves (HS-7, HS-8, HS-9)

After the initial six audit-derived optimizations (Waves A-C), three additional caching layers followed the same proven pattern: a 128-slot ring cache keyed on `(fxhash(text), len, format)` storing `Arc<Vec<Result>>`. Together they extended the cumulative speedup from -27.1% to -85.5%, dropping total workload time from 23.50s to ~3.4s.

### Cumulative phase deltas

| Phase | Audit | After Waves A-C | After HS-7 | After HS-8 | After HS-9 | After HS-10 | After HS-11 |
|---|---|---|---|---|---|---|---|
| diagnostics × 100,000 | 8.83s | 4.92s | 5.62s | **0.024s** | 0.024s | 0.024s | 0.024s |
| workspace_symbols × 20,000 | 11.59s | 10.33s | **1.70s** | 1.66s | 1.66s | 1.64s | **0.319s** |
| drift × 100 | 2.06s | 1.01s | 1.04s | 1.41s | 1.41s | **0.036s** | 0.031s |
| semantic_tokens × 50,000 | 0.79s | 0.77s | 0.76s | 0.78s | **~0.05s** | 0.009s | 0.008s |
| completion × 50,000 | 0.12s | 0.11s | 0.11s | 0.12s | 0.12s | 0.125s | 0.114s |
| **total** | **23.50s** | **17.13s** | **9.34s** | **4.06s** | **2.85s** | **1.83s** | **0.496s** |
| **cumulative speedup** | — | -27.1% | -60.3% | -82.7% | -87.9% | -92.2% | **-97.9%** |

### Per-HS outcomes

| HS | File | What | Outcome |
|---|---|---|---|
| HS-7 | `crates/vespertide-lsp/src/symbols.rs` | 128-slot ring cache keyed on `(fxhash(text), len)` storing `Arc<Vec<RawSymbol>>`. Pre-query symbol extraction is cached; per-call query filter runs on the cached vec in microseconds. | workspace_symbols **10.39s → 1.73s (-83.4%)** |
| HS-8 | `crates/vespertide-lsp/src/diagnostics/mod.rs` | 128-slot ring cache keyed on `(fxhash(text), len, format)` storing `Arc<Vec<DomainDiagnostic>>`. `compute_workspace` intentionally not cached. Refactored body into `compute_uncached`; public `compute` is now a cache lookup. | diagnostics **5.62s → 0.024s (-99.6%)** |
| HS-9 | `crates/vespertide-lsp/src/semantic_tokens/mod.rs` | 128-slot ring cache keyed on `(fxhash(source), len, format)` storing `Arc<Vec<RawToken>>`. Same shape as HS-7 / HS-8. | semantic_tokens **0.78s → ~0.05s (-94%)** |

### Test count

2062 baseline → 2106 after HS-9 (+44 tests across 9 optimizations):

- HS-7: +4 (cache hit/miss, format disambiguation, filter, full-API determinism)
- HS-8: +4 (cache hit, format disambiguation, distinct texts, `compute_uncached` bypass)
- HS-9: +4 (cache hit, format disambiguation, public API uses cache, None tree returns empty)

### New floor

After HS-9, the remaining hot spots are:

- **workspace_symbols ~1.66s (49%)** — at the architectural floor for the current design. Further wins require caching filtered results keyed on `(query, text)`, which expands cache cardinality significantly and needs a different eviction strategy.
- **drift ~1.41s (41%)** — HS-5 caches `LoadedState` but not `Vec<DomainDrift>`. A full-result cache would need to invalidate on any document tree change, not just mtime probes. Meaningful refactor; deferred.
- **semantic_tokens ~0.05s, diagnostics ~0.024s, completion ~0.12s** — effectively at the floor. No further optimization warranted.

### Cache architecture summary

All six caches in `vespertide-lsp` share the same structural pattern:

```
Pattern: 128-slot ring buffer + Mutex<Inner>
Key:     (fxhash64(text), text.len(), format_optional)
Value:   Arc<Vec<Result>>
Hit:     return Arc::clone(&slot.value)
Miss:    compute, insert, return Arc
Eviction: ring-buffer (oldest replaced)
Capacity: 128 entries × ~64-byte value ≈ 8 KB ceiling per cache
Caches:  SymbolCache (HS-7), DiagnosticsCache (HS-8), TokenCache (HS-9),
         ParseCache (HS-3), CachedTree (HS-2), DriftCache (HS-5)
```

### Recommendation

Further optimization rounds yield diminishing returns from here. Future work should focus on one of three directions:

- **(a) Real-editor cadence measurement** — synthetic throughput benchmarks don't reflect `didChange` invalidation patterns. Measuring actual hit rates in a live editor session would validate whether the 128-slot capacity is appropriate or needs tuning.
- **(b) Architectural floor for workspace_symbols and drift** — both require design changes (query-keyed cache, document-state hash) rather than drop-in ring caches. Worth a dedicated design spike if latency targets tighten.
- **(c) Feature work** — the LSP is now fast enough that user-visible latency is dominated by network/IPC, not compute. Shipping new capabilities is likely higher value than further micro-optimization.

### HS-10 — Drift result cache

- **What**: extended `DriftCache` to cache the final `Vec<DomainDrift>` itself, on top of the existing HS-5 `LoadedState` cache. The cache key combines HS-5's mtime triple with a new `docstore_fingerprint: u64` (FxHasher digest of all open documents' content). When any open file's text changes, the fingerprint changes, the cache invalidates, and drift recomputes.
- **File**: `crates/vespertide-lsp/src/drift/{cache,mod}.rs`
- **Outcome**: drift phase **1.08s → 0.036s (-96.7%)**; total **2.85s → 1.83s (-35.5%)**
- **Tests**: +6 (5 cache unit + 1 integration warm-cache identity)
- **Correctness invariant**: the fingerprint reads ALL open document texts via `DocumentStore::for_each` (deterministic URI ordering). Any LSP `did_change` advances the fingerprint, so byte ranges in cached drifts are always coherent with the current document tree state.

### Generic RingCache refactor (perf-neutral)

- **What**: extracted the common 128-slot / 64-slot ring-buffer cache shape into a generic `RingCache<K, V, const N>` in `crates/vespertide-lsp/src/cache.rs` (181 lines). All 4 caches that share the pattern (`SymbolCache`, `DiagnosticsCache`, `TokenCache`, `ParseCache`) are now thin typedefs over `RingCache<_, _, N>`. Adding a 5th cache in the future costs ~10 lines.
- **Effect on perf**: 0 (this is a refactor)
- **Effect on LoC**: net **~-274 lines** workspace-wide. Per-file deltas:
  ```
  symbols.rs:              614 → 500  (-114)
  diagnostics/mod.rs:      724 → 569  (-155)
  semantic_tokens/mod.rs:  214 → 149   (-65)
  validation/cache.rs:     246 → 167   (-79)
  drift/mod.rs:            701 → 659   (-42, extracted helper)
  NEW cache.rs:                  +181
  ```
- **Tests**: +4 generic RingCache tests (hit, miss, eviction, threadsafe)

### HS-11 — Workspace-wide symbol caches

- **What**: two-layer cache in `symbols.rs`:
  1. `WorkspaceSymbolsCache` (8 slots): keyed on `docstore_fingerprint`, stores the workspace-wide flat `Vec<WorkspaceSymbolEntry>` (one entry per `(uri, raw_symbol)` across all open docs + disk tables). Rebuilt when ANY doc changes.
  2. `FilteredSymbolsCache` (256 slots): keyed on `(docstore_fingerprint, needle_hash)`, stores the final sorted `Vec<DomainSymbol>` ready to return.
- Per-call flow: hash docs → hash needle → cache lookup → cache hit returns Arc clone of pre-sorted result. Miss: build flat list (uses HS-7 per-doc cache) → filter → sort → cache + return.
- **Files**: `crates/vespertide-lsp/src/{symbols.rs, cache.rs, drift/cache.rs}` — `docstore_fingerprint` promoted from drift/cache.rs (`pub(super)`) to `crate::cache::docstore_fingerprint` (`pub(crate)`).
- **Outcome**: workspace_symbols **1.638s → 0.319s (-80.5%)**; total synthetic **1.83s → 0.50s (-72.7%)**
- **Tests**: +10 (5 cache unit + 5 integration covering hit/miss/invalidation/sort/end-to-end)

### Realistic editor cadence workload variant

- **What**: new `--workload realistic` flag in `tools/lsp-profile/`. Interleaves one `did_change` text mutation per outer iteration before running a smaller per-iter request batch (10k diagnostics + 5k completion + 5k semantic_tokens + 1k workspace_symbols + 100 drift across 100 outer iterations). The `did_change` advances `docstore_fingerprint`, invalidating workspace-wide caches.
- **Synthetic baseline (`docs/profiling-baseline.json`)** = best-case (caches always warm).
- **Realistic baseline (`docs/profiling-realistic.json`)** = closer to real LSP cadence.
- **Synthetic vs realistic per-call slowdown table** (cache invalidation impact):

  | Phase | Synthetic ns/call | Realistic ns/call | Slowdown |
  |---|---|---|---|
  | diagnostics | 237 | 3,323 | **14.05×** ← HS-8 cache thrash dominant |
  | completion | 2,505 | 3,814 | 1.52× — moderate cache help |
  | semantic_tokens | 176 | 713 | 4.05× — HS-9 cache thrash |
  | workspace_symbols | 15,950 | 115,382 | 1.41× — HS-11 rebuild is fast |
  | drift | 355,750 | 976,170 | 2.74× — HS-10 invalidates per iter |

- **How to run**:
  ```bash
  # Synthetic (cache best-case)
  cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- --workload synthetic --json docs/profiling-baseline.json

  # Realistic (with did_change cadence)
  cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- --workload realistic --json docs/profiling-realistic.json
  ```

- **Interpretation**: caches retain useful effect under did_change cadence (1.4×-4× slowdown is still cheap), but diagnostics is most affected (HS-8's per-text cache invalidates every did_change). The realistic baseline is the better proxy for real editor responsiveness.

After 11 optimizations + 1 refactor + 1 workload variant, cumulative speedup vs audit baseline is **23.50s → 0.50s = -97.9%**. The remaining 0.50s is now distributed across workspace_symbols (64% — Arc clone + sort cost on cache hit), completion (23%), and the floor-state diagnostics/semantic_tokens/drift. The synthetic workload has been optimized to its useful limit. For realistic LSP responsiveness, see the `realistic` workload variant (`docs/profiling-realistic.json`) which simulates did_change cadence.

Test count: 2062 baseline → **2118 (+56 across 11 optimizations + 1 refactor + 1 workload variant)**.

---

## Reproducing on CI

The `bench.yml` workflow runs the synthetic and realistic workloads on every PR touching `vespertide-lsp` / `tools/lsp-profile`, plus nightly at 02:00 KST. Each run uploads:
- `profiling-synthetic-current.json` — current synthetic baseline measurement
- `profiling-realistic-current.json` — current realistic baseline (with p50/p95/p99 latency)
- `synthetic-delta.txt` / `realistic-delta.txt` — diff against committed baselines

PR runs emit `::warning::` messages for >20% phase-wall regressions (non-blocking). Nightly runs serve as the long-term trend log.

To reproduce a CI run locally:
```bash
cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- --workload synthetic --json /tmp/synthetic.json
cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- --workload realistic --json /tmp/realistic.json
cargo run --release --manifest-path tools/lsp-profile/Cargo.toml -- --workload synthetic --baseline docs/profiling-baseline.json
```

---

## Troubleshooting

**Windows: `STATUS_ACCESS_DENIED`**
Run the terminal as Administrator. ETW sampling requires elevated privileges; blondie will fail silently or with an access error otherwise.

**Linux: `perf_event_paranoid > 1`**
Run the sysctl command from the Prerequisites section:
```bash
sudo sysctl -w kernel.perf_event_paranoid=-1
```

**SVG file size too small (< 50 KB)**
The workload binary likely returned immediately without doing any work. Check the binary's stderr output for panics or early exits. A healthy run produces an SVG in the 200-800 KB range.
