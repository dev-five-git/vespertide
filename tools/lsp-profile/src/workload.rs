//! Runs vespertide-lsp through 5 hot-path scenarios on the synthetic
//! workspace. Phase timings are returned as structured data; profilers capture
//! function-level data separately.

use crate::fixture::Scenario;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::time::{Duration, Instant};
use tower_lsp_server::ls_types::Uri;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, WorkspaceTables, compute_completion,
    compute_diagnostics_shared, compute_drift, compute_workspace_symbols_shared, semantic_tokens,
    uri_to_path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyStats {
    pub min_us: f64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
    pub mean_us: f64,
    pub samples: usize,
}

impl LatencyStats {
    pub fn from_samples(durations: &mut [Duration]) -> Self {
        if durations.is_empty() {
            return Self {
                min_us: 0.0,
                p50_us: 0.0,
                p95_us: 0.0,
                p99_us: 0.0,
                max_us: 0.0,
                mean_us: 0.0,
                samples: 0,
            };
        }

        durations.sort();
        let samples = durations.len();
        let sum_us = durations
            .iter()
            .map(|duration| duration_us(*duration))
            .sum::<f64>();
        let sample_count = u32::try_from(samples).map_or(f64::from(u32::MAX), f64::from);

        Self {
            min_us: duration_us(durations[0]),
            p50_us: percentile_us(durations, 50, 100),
            p95_us: percentile_us(durations, 95, 100),
            p99_us: percentile_us(durations, 99, 100),
            max_us: duration_us(durations[samples - 1]),
            mean_us: sum_us / sample_count,
            samples,
        }
    }
}

fn percentile_us(durations: &[Duration], numerator: usize, denominator: usize) -> f64 {
    let samples = durations.len();
    let index = (samples.saturating_mul(numerator) / denominator).min(samples - 1);
    duration_us(durations[index])
}

fn duration_us(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseTiming {
    pub name: String,
    pub calls: usize,
    pub wall_secs: f64,
    pub items: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencyStats>,
}

const WORKSPACE_SYMBOL_QUERIES: [&str; 10] = [
    "",
    "table",
    "id",
    "name",
    "status",
    "email",
    "user",
    "created",
    "tag",
    "parent_001",
];

/// Outer iterations multiplier. Calibrated so total wall time lands at
/// ~30-50s on a typical developer laptop — enough for cargo-flamegraph's
/// default 99 Hz sampler to capture ~3-5k statistically meaningful stack
/// samples. Each phase keeps a constant proportional weight regardless
/// of ITERATIONS so the flame graph composition stays comparable across
/// runs even when this constant changes.
const ITERATIONS: usize = 1000;
const REALISTIC_ITERATIONS: usize = 100;
const REALISTIC_REQUEST_DOCS: usize = 50;

struct PhaseAccumulator {
    name: &'static str,
    calls: usize,
    wall_secs: f64,
    items: usize,
}

impl PhaseAccumulator {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            calls: 0,
            wall_secs: 0.0,
            items: 0,
        }
    }

    fn record(&mut self, started: Instant, calls: usize, items: usize) {
        self.calls += calls;
        self.wall_secs += started.elapsed().as_secs_f64();
        self.items += items;
    }

    fn finish(self) -> PhaseTiming {
        PhaseTiming {
            name: self.name.to_string(),
            calls: self.calls,
            wall_secs: self.wall_secs,
            items: self.items,
            latency: None,
        }
    }
}

fn finish_with_latency(accumulator: PhaseAccumulator, samples: &mut [Duration]) -> PhaseTiming {
    let mut timing = accumulator.finish();
    timing.latency = Some(LatencyStats::from_samples(samples));
    timing
}

pub fn run(scenario: &Scenario) -> Result<Vec<PhaseTiming>> {
    let docs = DocumentStore::new();
    let index = WorkspaceIndex::new();
    let disk_tables = WorkspaceTables::new();
    let parser_pool = ParserPool::new();

    disk_tables.refresh(scenario.root.as_path());
    open_models(scenario, &docs, &index, &parser_pool)?;

    let opened_uris = docs.open_uris();
    let timings = vec![
        run_diagnostics_phase(&docs, &opened_uris),
        run_completion_phase(&docs, &index, &opened_uris),
        run_semantic_tokens_phase(&docs, &opened_uris),
        run_workspace_symbols_phase(&docs, &disk_tables),
        run_drift_phase(scenario, &docs, &index),
    ];

    Ok(timings)
}

struct RealisticMetrics {
    diagnostics: PhaseAccumulator,
    completion: PhaseAccumulator,
    semantic_tokens: PhaseAccumulator,
    workspace_symbols: PhaseAccumulator,
    drift: PhaseAccumulator,
    did_change_to_first_request: PhaseAccumulator,
    diagnostics_samples: Vec<Duration>,
    completion_samples: Vec<Duration>,
    semantic_tokens_samples: Vec<Duration>,
    workspace_symbols_samples: Vec<Duration>,
    drift_samples: Vec<Duration>,
    did_change_to_first_request_samples: Vec<Duration>,
}

impl RealisticMetrics {
    fn new(opened_uri_count: usize, request_doc_count: usize) -> Self {
        Self {
            diagnostics: PhaseAccumulator::new("diagnostics"),
            completion: PhaseAccumulator::new("completion"),
            semantic_tokens: PhaseAccumulator::new("semantic_tokens"),
            workspace_symbols: PhaseAccumulator::new("workspace_symbols"),
            drift: PhaseAccumulator::new("drift"),
            did_change_to_first_request: PhaseAccumulator::new("did_change_to_first_request"),
            diagnostics_samples: Vec::with_capacity(REALISTIC_ITERATIONS * opened_uri_count),
            completion_samples: Vec::with_capacity(REALISTIC_ITERATIONS * request_doc_count),
            semantic_tokens_samples: Vec::with_capacity(REALISTIC_ITERATIONS * request_doc_count),
            workspace_symbols_samples: Vec::with_capacity(
                REALISTIC_ITERATIONS * WORKSPACE_SYMBOL_QUERIES.len(),
            ),
            drift_samples: Vec::with_capacity(REALISTIC_ITERATIONS),
            did_change_to_first_request_samples: Vec::with_capacity(REALISTIC_ITERATIONS),
        }
    }

    fn finish(self) -> Vec<PhaseTiming> {
        let Self {
            diagnostics,
            completion,
            semantic_tokens,
            workspace_symbols,
            drift,
            mut did_change_to_first_request,
            mut diagnostics_samples,
            mut completion_samples,
            mut semantic_tokens_samples,
            mut workspace_symbols_samples,
            mut drift_samples,
            mut did_change_to_first_request_samples,
        } = self;

        did_change_to_first_request.calls = did_change_to_first_request_samples.len();
        did_change_to_first_request.wall_secs = did_change_to_first_request_samples
            .iter()
            .map(Duration::as_secs_f64)
            .sum();

        vec![
            finish_with_latency(diagnostics, &mut diagnostics_samples),
            finish_with_latency(completion, &mut completion_samples),
            finish_with_latency(semantic_tokens, &mut semantic_tokens_samples),
            finish_with_latency(workspace_symbols, &mut workspace_symbols_samples),
            finish_with_latency(drift, &mut drift_samples),
            finish_with_latency(
                did_change_to_first_request,
                &mut did_change_to_first_request_samples,
            ),
        ]
    }
}

pub fn run_realistic(scenario: &Scenario) -> Result<Vec<PhaseTiming>> {
    let docs = DocumentStore::new();
    let index = WorkspaceIndex::new();
    let disk_tables = WorkspaceTables::new();
    let parser_pool = ParserPool::new();

    disk_tables.refresh(scenario.root.as_path());
    open_models(scenario, &docs, &index, &parser_pool)?;

    let opened_uris = docs.open_uris();
    let request_doc_count = opened_uris.len().min(REALISTIC_REQUEST_DOCS);
    let mut metrics = RealisticMetrics::new(opened_uris.len(), request_doc_count);

    if !opened_uris.is_empty() {
        for iter in 0..REALISTIC_ITERATIONS {
            let target_uri = &opened_uris[iter % opened_uris.len()];
            let current_text = docs
                .with_text(target_uri, ToString::to_string)
                .with_context(|| format!("read open model text: {}", target_uri.as_str()))?;
            let mutated = format!("{current_text} ");
            let version = i32::try_from(iter).context("realistic iteration overflow")? + 2;
            let did_change_started = Instant::now();
            update_doc_and_index(target_uri, &mutated, version, &docs, &index, &parser_pool)?;

            record_realistic_diagnostics(
                &docs,
                &opened_uris,
                &mut metrics,
                did_change_started,
            );
            record_realistic_completion(
                &docs,
                &index,
                &opened_uris,
                request_doc_count,
                &mut metrics,
            );
            record_realistic_semantic_tokens(&docs, &opened_uris, request_doc_count, &mut metrics);
            record_realistic_workspace_symbols(&docs, &disk_tables, &mut metrics);
            record_realistic_drift(scenario, &docs, &index, &mut metrics);
        }
    }

    Ok(metrics.finish())
}

fn record_realistic_diagnostics(
    docs: &DocumentStore,
    opened_uris: &[Uri],
    metrics: &mut RealisticMetrics,
    did_change_started: Instant,
) {
    let started = Instant::now();
    let mut first_request_recorded = false;
    let mut diagnostic_count = 0;

    for uri in opened_uris {
        let call_started = Instant::now();
        let items = docs
            .docs_iter_for_uri(uri, |state| {
                compute_diagnostics_shared(state.text(), state.format, state.tree.as_ref())
                    .len()
            })
            .unwrap_or_default();

        metrics.diagnostics_samples.push(call_started.elapsed());
        if !first_request_recorded {
            metrics
                .did_change_to_first_request_samples
                .push(did_change_started.elapsed());
            first_request_recorded = true;
        }
        diagnostic_count += items;
    }

    metrics
        .diagnostics
        .record(started, opened_uris.len(), diagnostic_count);
}

fn record_realistic_completion(
    docs: &DocumentStore,
    index: &WorkspaceIndex,
    opened_uris: &[Uri],
    request_doc_count: usize,
    metrics: &mut RealisticMetrics,
) {
    let started = Instant::now();
    let mut item_count = 0;

    for uri in opened_uris.iter().take(request_doc_count) {
        let call_started = Instant::now();
        let items = docs
            .docs_iter_for_uri(uri, |state| {
                let byte_offset = state.text().len() / 2;
                compute_completion(
                    state.text(),
                    state.format,
                    state.tree.as_ref(),
                    index,
                    docs,
                    byte_offset,
                )
                .len()
            })
            .unwrap_or_default();

        metrics.completion_samples.push(call_started.elapsed());
        item_count += items;
    }

    metrics
        .completion
        .record(started, request_doc_count, item_count);
}

fn record_realistic_semantic_tokens(
    docs: &DocumentStore,
    opened_uris: &[Uri],
    request_doc_count: usize,
    metrics: &mut RealisticMetrics,
) {
    let started = Instant::now();
    let mut token_count = 0;

    for uri in opened_uris.iter().take(request_doc_count) {
        let call_started = Instant::now();
        let items = docs
            .docs_iter_for_uri(uri, |state| {
                semantic_tokens::classify_shared(state.text(), state.format, state.tree.as_ref())
                    .len()
            })
            .unwrap_or_default();

        metrics.semantic_tokens_samples.push(call_started.elapsed());
        token_count += items;
    }

    metrics
        .semantic_tokens
        .record(started, request_doc_count, token_count);
}

fn record_realistic_workspace_symbols(
    docs: &DocumentStore,
    disk_tables: &WorkspaceTables,
    metrics: &mut RealisticMetrics,
) {
    let started = Instant::now();
    let mut item_count = 0;

    for query in WORKSPACE_SYMBOL_QUERIES {
        let call_started = Instant::now();
        item_count += compute_workspace_symbols_shared(query, docs, Some(disk_tables)).len();
        metrics
            .workspace_symbols_samples
            .push(call_started.elapsed());
    }

    metrics
        .workspace_symbols
        .record(started, WORKSPACE_SYMBOL_QUERIES.len(), item_count);
}

fn record_realistic_drift(
    scenario: &Scenario,
    docs: &DocumentStore,
    index: &WorkspaceIndex,
    metrics: &mut RealisticMetrics,
) {
    let started = Instant::now();
    let drift_items = compute_drift(&scenario.root, index, docs).len();
    metrics.drift_samples.push(started.elapsed());
    metrics.drift.record(started, 1, drift_items);
}

fn open_models(
    scenario: &Scenario,
    docs: &DocumentStore,
    index: &WorkspaceIndex,
    parser_pool: &ParserPool,
) -> Result<()> {
    for uri_text in &scenario.model_uris {
        let uri: Uri = uri_text
            .parse()
            .with_context(|| format!("parse model URI: {uri_text}"))?;
        let path =
            uri_to_path(&uri).with_context(|| format!("convert URI to path: {}", uri.as_str()))?;
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read model file: {}", path.display()))?;
        let tree = parser_pool
            .parse(&text, DocumentFormat::Json)
            .with_context(|| format!("parse model file: {}", path.display()))?;

        index.upsert(&uri, &text, &tree);
        docs.open(uri, "json".to_string(), 1, text);
    }

    Ok(())
}

fn update_doc_and_index(
    uri: &Uri,
    new_text: &str,
    version: i32,
    docs: &DocumentStore,
    index: &WorkspaceIndex,
    parser_pool: &ParserPool,
) -> Result<()> {
    docs.update_full(uri, new_text.to_string(), version);
    let tree = parser_pool
        .parse(new_text, DocumentFormat::Json)
        .context("re-parse after did_change")?;
    index.upsert(uri, new_text, &tree);
    Ok(())
}

fn run_diagnostics_phase(docs: &DocumentStore, opened_uris: &[Uri]) -> PhaseTiming {
    let started = Instant::now();
    let mut diagnostic_count = 0;

    for _ in 0..ITERATIONS {
        for uri in opened_uris {
            diagnostic_count += docs
                .docs_iter_for_uri(uri, |state| {
                    compute_diagnostics_shared(state.text(), state.format, state.tree.as_ref())
                        .len()
                })
                .unwrap_or_default();
        }
    }

    let diagnostic_runs = opened_uris.len() * ITERATIONS;
    let elapsed = started.elapsed().as_secs_f64();
    PhaseTiming {
        name: "diagnostics".to_string(),
        calls: diagnostic_runs,
        wall_secs: elapsed,
        items: diagnostic_count,
        latency: None,
    }
}

fn run_completion_phase(
    docs: &DocumentStore,
    index: &WorkspaceIndex,
    opened_uris: &[Uri],
) -> PhaseTiming {
    let started = Instant::now();
    let per_iter = opened_uris.len() / 2;
    let mut item_count = 0;

    for _ in 0..ITERATIONS {
        for uri in opened_uris.iter().take(per_iter) {
            item_count += docs
                .docs_iter_for_uri(uri, |state| {
                    let byte_offset = state.text().len() / 2;
                    compute_completion(
                        state.text(),
                        state.format,
                        state.tree.as_ref(),
                        index,
                        docs,
                        byte_offset,
                    )
                    .len()
                })
                .unwrap_or_default();
        }
    }

    let completion_runs = per_iter * ITERATIONS;
    let elapsed = started.elapsed().as_secs_f64();
    PhaseTiming {
        name: "completion".to_string(),
        calls: completion_runs,
        wall_secs: elapsed,
        items: item_count,
        latency: None,
    }
}

fn run_semantic_tokens_phase(docs: &DocumentStore, opened_uris: &[Uri]) -> PhaseTiming {
    let started = Instant::now();
    let per_iter = opened_uris.len() / 2;
    let mut token_count = 0;

    for _ in 0..ITERATIONS {
        for uri in opened_uris.iter().take(per_iter) {
            token_count += docs
                .docs_iter_for_uri(uri, |state| {
                    semantic_tokens::classify_shared(state.text(), state.format, state.tree.as_ref())
                        .len()
                })
                .unwrap_or_default();
        }
    }

    let token_runs = per_iter * ITERATIONS;
    let elapsed = started.elapsed().as_secs_f64();
    PhaseTiming {
        name: "semantic_tokens".to_string(),
        calls: token_runs,
        wall_secs: elapsed,
        items: token_count,
        latency: None,
    }
}

fn run_workspace_symbols_phase(docs: &DocumentStore, disk_tables: &WorkspaceTables) -> PhaseTiming {
    let started = Instant::now();
    let mut item_count = 0;

    for _ in 0..(ITERATIONS * 2) {
        for query in WORKSPACE_SYMBOL_QUERIES {
            item_count += compute_workspace_symbols_shared(query, docs, Some(disk_tables)).len();
        }
    }

    let symbol_runs = WORKSPACE_SYMBOL_QUERIES.len() * 2 * ITERATIONS;
    let elapsed = started.elapsed().as_secs_f64();
    PhaseTiming {
        name: "workspace_symbols".to_string(),
        calls: symbol_runs,
        wall_secs: elapsed,
        items: item_count,
        latency: None,
    }
}

fn run_drift_phase(
    scenario: &Scenario,
    docs: &DocumentStore,
    index: &WorkspaceIndex,
) -> PhaseTiming {
    let started = Instant::now();
    // drift is already ~70% of total wall time at the base count of 20 —
    // a 5× multiplier is enough to keep its share dominant without
    // ballooning total runtime past 60s on a typical laptop.
    let drift_runs = 20 * 5;
    let mut drift_count = 0;

    for _ in 0..drift_runs {
        drift_count += compute_drift(&scenario.root, index, docs).len();
    }

    let elapsed = started.elapsed().as_secs_f64();
    PhaseTiming {
        name: "drift".to_string(),
        calls: drift_runs,
        wall_secs: elapsed,
        items: drift_count,
        latency: None,
    }
}

#[cfg(test)]
mod tests {
    use crate::fixture;
    use std::time::Duration;

    use super::*;

    #[test]
    fn latency_stats_from_samples_computes_percentiles() {
        let mut samples = (1..=100).map(Duration::from_micros).collect::<Vec<_>>();

        let stats = LatencyStats::from_samples(&mut samples);

        assert_eq!(stats.samples, 100);
        assert!((stats.min_us - 1.0).abs() < 0.001);
        assert!((stats.max_us - 100.0).abs() < 0.001);
        assert!(stats.p50_us >= 50.0 && stats.p50_us <= 51.0);
        assert!(stats.p95_us >= 95.0 && stats.p95_us <= 96.0);
        assert!(stats.p99_us >= 99.0 && stats.p99_us <= 100.0);
        assert!((stats.mean_us - 50.5).abs() < 0.1);
    }

    #[test]
    fn latency_stats_handles_empty() {
        let mut empty = Vec::new();

        let stats = LatencyStats::from_samples(&mut empty);

        assert_eq!(stats.samples, 0);
        assert!(stats.p50_us.abs() < f64::EPSILON);
    }

    #[test]
    fn run_executes_against_small_synthetic_workspace() {
        let scenario = fixture::build_workspace(4).expect("workspace fixture should build");

        let timings = run(&scenario).expect("workload should complete");
        assert_eq!(timings.len(), 5);
    }

    #[test]
    fn run_realistic_executes_against_small_synthetic_workspace() {
        let scenario = fixture::build_workspace(4).expect("build");

        let timings = run_realistic(&scenario).expect("realistic run");
        assert_eq!(timings.len(), 6, "5 phases plus latency headline timing");
        for timing in &timings {
            assert!(timing.calls > 0, "phase {} had no calls", timing.name);
        }
    }

    #[test]
    fn run_realistic_emits_did_change_to_first_request_phase() {
        let scenario = fixture::build_workspace(4).expect("build");

        let timings = run_realistic(&scenario).expect("realistic run");

        assert_eq!(timings.len(), 6, "5 phases + did_change_to_first_request");
        let dctfr = timings
            .iter()
            .find(|timing| timing.name == "did_change_to_first_request")
            .expect("did_change_to_first_request entry");
        let latency = dctfr.latency.as_ref().expect("latency stats");
        assert!(latency.samples > 0);
    }

    #[test]
    fn phase_timings_serialize_to_json() {
        let scenario = fixture::build_workspace(4).expect("build");
        let timings = run(&scenario).expect("run");
        assert_eq!(timings.len(), 5);
        let json = serde_json::to_string(&timings).expect("serialize");
        assert!(json.contains("diagnostics"));
        assert!(json.contains("wall_secs"));
    }
}
