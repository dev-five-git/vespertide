//! Fuzz the pure-domain LSP request helpers. Arbitrary input should
//! either produce best-effort results or no result, but never panic.
//!
//! Capabilities exercised (panic-safety only — outputs are discarded):
//!
//! * `diagnostics`         — `compute_diagnostics`
//! * `formatting`          — `format_text`
//! * `hover`               — `compute_hover`
//! * `definition`          — `compute_definition`
//! * `completion`          — `compute_completion`,
//!   `compute_completion_with_workspace_tables`
//! * `references`          — `compute_references`
//! * `rename`              — `compute_rename`, `prepare_rename`
//! * `code_actions`        — `compute_code_actions`
//! * `inlay_hints`         — `compute_inlay_hints`
//! * `workspace_symbols`   — `compute_workspace_symbols`
//! * `document_symbol`     — `compute_document_symbols`
//! * `folding_range`       — `compute_folding_ranges`
//! * `document_highlight`  — `compute_document_highlight`
//! * `selection_range`     — `compute_selection_ranges`
//! * `semantic_tokens`     — `semantic_tokens::classify` +
//!   `semantic_tokens::filter_range` (full + range filter)
//! * `watched_files`       — `watched_files::should_refresh_for`
//!
//! Strategy: for every capability that consumes a cursor offset or a
//! byte range, throw multiple offsets derived from `data` itself
//! (deterministic from libfuzzer's perspective) plus boundary offsets
//! (`0`, `len/4`, `len/2`, `3*len/4`, `len`, `len + 1` past-the-end).
//! Ranges are formed by pairing two such offsets in both `[lo, hi]`
//! and `[hi, lo]` order so we also cover the inverted-range edge case.

#![no_main]

use std::path::PathBuf;
use std::str::FromStr;

use libfuzzer_sys::fuzz_target;
use tower_lsp_server::ls_types::Uri;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, ParserPool, WorkspaceIndex, compute_code_actions,
    compute_completion, compute_completion_with_workspace_tables, compute_definition,
    compute_diagnostics, compute_document_highlight, compute_document_symbols,
    compute_folding_ranges, compute_hover, compute_inlay_hints, compute_references, compute_rename,
    compute_selection_ranges, compute_workspace_symbols, format_text, prepare_rename,
    semantic_tokens::{
        classify as classify_semantic_tokens, filter_range as filter_semantic_tokens,
    },
    watched_files::should_refresh_for,
};

/// Derive a usize from a 4-byte window of `data`, modulo `(len + 1)` so
/// the result is always a legal "past-the-end" cursor position.
fn derived_offset(data: &[u8], start: usize, len: usize) -> usize {
    if data.len() < start + 4 {
        return 0;
    }
    let n = u32::from_le_bytes([
        data[start],
        data[start + 1],
        data[start + 2],
        data[start + 3],
    ]) as usize;
    n % (len + 1)
}

/// Build a set of cursor offsets to exercise: structured boundary
/// values plus three offsets derived from arbitrary `data` bytes.
fn cursor_offsets(data: &[u8], len: usize) -> [usize; 8] {
    [
        0,
        len / 4,
        len / 2,
        3 * len / 4,
        len,
        len.saturating_add(1), // past-the-end probe
        derived_offset(data, 0, len),
        derived_offset(data, 4, len),
    ]
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    let text_len = text.len();

    // Stable URI used as `current_uri` for cross-file capabilities. The
    // workspace store will contain this single document so references /
    // rename actually have somewhere to look.
    let uri = Uri::from_str("file:///fuzz/models/fuzz.json").expect("static URI parses");

    for format in [DocumentFormat::Json, DocumentFormat::Yaml] {
        let pool = ParserPool::new();
        let tree = pool.parse(&text, format);
        let index = WorkspaceIndex::new();
        let docs = DocumentStore::new();

        // Register the document in the store + index so workspace_symbols,
        // references, rename, etc. have a live target to walk.
        let language_id = match format {
            DocumentFormat::Json => "json",
            DocumentFormat::Yaml => "yaml",
        };
        docs.open(uri.clone(), language_id.to_string(), 1, text.clone());
        if let Some(t) = tree.as_ref() {
            let _ = index.upsert(&uri, &text, t);
        }

        // -------------------------------------------------------------
        // Cursor-free capabilities
        // -------------------------------------------------------------
        let _ = compute_diagnostics(&text, format, tree.as_ref(), &index);
        let _ = format_text(&text, format);
        let _ = compute_document_symbols(&text, tree.as_ref());
        let _ = compute_folding_ranges(&text, tree.as_ref());

        // workspace_symbols: empty query + a query derived from input.
        let _ = compute_workspace_symbols("", &docs, None);
        let derived_query: String = text.chars().take(8).collect();
        let _ = compute_workspace_symbols(&derived_query, &docs, None);

        // semantic_tokens full classify (range filter is exercised below).
        let full_tokens = classify_semantic_tokens(&text, format, tree.as_ref());
        let _ = full_tokens; // discard but keep the call live.

        // -------------------------------------------------------------
        // Cursor-driven capabilities
        // -------------------------------------------------------------
        let offsets = cursor_offsets(data, text_len);
        for &byte_offset in &offsets {
            let _ = compute_hover(&text, format, tree.as_ref(), &index, &docs, byte_offset);
            let _ = compute_definition(&text, format, tree.as_ref(), &index, &docs, byte_offset);
            let _ = compute_completion(&text, format, tree.as_ref(), &index, &docs, byte_offset);
            let _ = compute_document_highlight(&text, tree.as_ref(), byte_offset);
            let _ = compute_selection_ranges(&text, tree.as_ref(), byte_offset);

            // references — both include and exclude declaration variants.
            let _ = compute_references(
                &text,
                format,
                tree.as_ref(),
                &uri,
                &index,
                &docs,
                None,
                byte_offset,
                true,
            );
            let _ = compute_references(
                &text,
                format,
                tree.as_ref(),
                &uri,
                &index,
                &docs,
                None,
                byte_offset,
                false,
            );

            // prepare_rename + rename. Use a deterministic-but-derived
            // new name so we exercise the identifier validator with
            // varying lengths instead of a constant.
            let _ = prepare_rename(&text, format, tree.as_ref(), &uri, byte_offset);
            let new_name_seed = data
                .iter()
                .fold(0u32, |acc, b| acc.wrapping_add(u32::from(*b)));
            let new_name = format!("renamed_{new_name_seed:x}");
            let _ = compute_rename(
                &text,
                format,
                tree.as_ref(),
                &uri,
                &index,
                &docs,
                None,
                byte_offset,
                &new_name,
            );
            // Empty rename — must safely return `None`, not panic.
            let _ = compute_rename(
                &text,
                format,
                tree.as_ref(),
                &uri,
                &index,
                &docs,
                None,
                byte_offset,
                "",
            );
        }

        // completion_with_workspace_tables exercises the same code path
        // as compute_completion but threads through the optional disk
        // table cache. Empty disk cache is fine for panic-safety.
        let disk = vespertide_lsp::WorkspaceTables::new();
        for &byte_offset in &offsets {
            let _ = compute_completion_with_workspace_tables(
                &text,
                format,
                tree.as_ref(),
                &index,
                &docs,
                &disk,
                byte_offset,
            );
        }

        // -------------------------------------------------------------
        // Range-driven capabilities: code_actions, inlay_hints,
        // semantic_tokens range filter.
        // -------------------------------------------------------------
        // Pair offsets to form ranges. Includes naturally-ordered
        // `[lo..hi]` and explicitly inverted `[hi..lo]` ranges (which
        // some implementations treat as empty — must not panic).
        for &lo in &offsets {
            for &hi in &offsets {
                let forward = lo.min(hi)..lo.max(hi);
                let _ = compute_code_actions(&text, format, tree.as_ref(), forward.clone());
                let _ = compute_inlay_hints(&text, tree.as_ref(), forward.clone());

                let tokens = classify_semantic_tokens(&text, format, tree.as_ref());
                let _ = filter_semantic_tokens(tokens, forward.clone());

                // Inverted range — should produce no actions / no hints,
                // but must NOT panic on the integer arithmetic.
                let inverted = lo.max(hi)..lo.min(hi);
                let _ = compute_code_actions(&text, format, tree.as_ref(), inverted.clone());
                let _ = compute_inlay_hints(&text, tree.as_ref(), inverted.clone());

                let tokens = classify_semantic_tokens(&text, format, tree.as_ref());
                let _ = filter_semantic_tokens(tokens, inverted);
            }
        }
    }

    // -------------------------------------------------------------
    // watched_files: path-based capability. Throw arbitrary bytes as
    // a path component to exercise canonicalisation + prefix logic.
    // -------------------------------------------------------------
    let candidate: PathBuf = PathBuf::from(String::from_utf8_lossy(data).as_ref());
    let root = PathBuf::from("/fuzz");
    let models = root.join("models");
    let migrations = root.join("migrations");

    let _ = should_refresh_for(&root, &models, &migrations, &candidate);
    // Also probe with paths obviously *inside* the tracked dirs, built
    // from arbitrary bytes — exercises the prefix-match success branch.
    let inside_models = models.join(String::from_utf8_lossy(data).as_ref());
    let _ = should_refresh_for(&root, &models, &migrations, &inside_models);
    let inside_migrations = migrations.join(String::from_utf8_lossy(data).as_ref());
    let _ = should_refresh_for(&root, &models, &migrations, &inside_migrations);
});
