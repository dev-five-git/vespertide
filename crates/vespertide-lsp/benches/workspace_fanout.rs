//! Benchmark the per-`did_change` diagnostics fan-out.
//!
//! A single `did_change` runs `publish` + `publish_related`, so a workspace
//! with `N` open documents triggers `N` calls to `Backend::collect_workspace_tables`
//! (one per published document). Before the workspace-table cache, each of
//! those calls re-deserialized + re-normalized + re-cloned every open model,
//! making one keystroke O(N²). The cache turns the fan-out into 1 build +
//! (N-1) hits — O(N).
//!
//! This bench drives the REAL `Backend` through the public `LanguageServer`
//! API and measures one `did_change` as a function of `N`, so the number it
//! reports is the actual user-visible per-keystroke latency.
//!
//! Run:    cargo bench -p vespertide-lsp --bench workspace_fanout
//! Compare: `--save-baseline <name>` / `--baseline <name>` (criterion).
//!
//! To measure the pre-cache baseline, make `collect_workspace_tables` bypass
//! the cache (early `return Arc::new(self.build_workspace_tables());`) and
//! re-run with a second `--save-baseline`.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use futures::StreamExt;
use serde_json::json;
use tower_lsp_server::ls_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, InitializeParams,
};
use tower_lsp_server::{LanguageServer, LspService};
use vespertide_lsp::Backend;

/// A valid `TableDef` JSON for document `i`. `variant` toggles a trailing
/// column so the docstore fingerprint actually changes on each measured edit
/// (otherwise every `did_change` would be a pure cache hit and not represent a
/// real keystroke).
fn model_json(i: usize, variant: u8) -> String {
    let note = if variant == 0 {
        ""
    } else {
        r#",{"name":"note","type":"text","nullable":true}"#
    };
    format!(
        r#"{{"name":"t{i}","columns":[{{"name":"id","type":"integer","nullable":false,"primary_key":true}},{{"name":"label","type":"text","nullable":false}}{note}]}}"#
    )
}

fn did_open(i: usize) -> DidOpenTextDocumentParams {
    serde_json::from_value(json!({
        "textDocument": {
            "uri": format!("file:///bench/t{i}.json"),
            "languageId": "json",
            "version": 1,
            "text": model_json(i, 0),
        }
    }))
    .expect("did_open params")
}

fn did_change(version: i32, variant: u8) -> DidChangeTextDocumentParams {
    serde_json::from_value(json!({
        "textDocument": { "uri": "file:///bench/t0.json", "version": version },
        "contentChanges": [{ "text": model_json(0, variant) }],
    }))
    .expect("did_change params")
}

fn bench_did_change_fanout(c: &mut Criterion) {
    // Current-thread runtime: the bounded client `channel(1)` drainer then runs
    // cooperatively during each `block_on` await instead of on a separate
    // worker thread, eliminating cross-thread wakeup latency (~ms on Windows
    // when the worker parks) that otherwise contaminates the measurement.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let mut group = c.benchmark_group("did_change_fanout");

    for &n in &[5usize, 10, 25, 50, 100] {
        let (service, socket) = LspService::new(Backend::new);
        // Drain server→client messages: the client channel is bounded
        // (mpsc::channel(1)), so without a consumer `publish_diagnostics`
        // would block once full and the fan-out would deadlock.
        rt.spawn(async move {
            let mut socket = socket;
            while socket.next().await.is_some() {}
        });
        let backend = service.inner();

        rt.block_on(async {
            let init: InitializeParams =
                serde_json::from_value(json!({ "capabilities": {} })).expect("init params");
            let _ = backend.initialize(init).await;
            for i in 0..n {
                backend.did_open(did_open(i)).await;
            }
        });

        let mut version = 2i32;
        let mut variant = 0u8;
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                variant ^= 1;
                let params = did_change(version, variant);
                version += 1;
                rt.block_on(backend.did_change(params));
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_did_change_fanout);
criterion_main!(benches);
