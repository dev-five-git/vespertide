//! Body of the `prepare_rename` / `rename` LSP handlers, lifted out of
//! `backend::mod` to keep the latter under the 1000-line per-file
//! policy. Each function takes `&Backend` so it can read the shared
//! state (store, index, workspace tables) without exposing fields.
//!
//! `async` is preserved on both helpers — the LSP trait expects an
//! `async fn`, and these wrappers must be `.await`-able from the trait
//! impl block in `mod.rs`. Clippy's `unused_async` lint fires here
//! because the bodies themselves don't `.await`, but removing `async`
//! would break the trait signature mirror. Expect the lint locally.
#![expect(
    clippy::unused_async,
    reason = "tower-lsp-server prepareRename/rename trait wrappers force async helpers even though these bodies are synchronous"
)]

use std::collections::{BTreeMap, HashMap};

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    Position, PrepareRenameResponse, Range, RenameParams, TextDocumentPositionParams, Uri,
    WorkspaceEdit,
};

use super::Backend;
use super::helpers::{byte_range_to_ls, domain_edits_to_lsp};
use crate::parser::DocumentFormat;

#[cfg(not(tarpaulin_include))]
fn log_prepare_rename_not_renameable(uri: &Uri) {
    tracing::debug!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        "prepare_rename: position is not renameable"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_prepare_rename(uri: &Uri, placeholder: &str) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        placeholder = %placeholder,
        "prepare_rename"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_rename(uri: &Uri, new_name: &str, files: usize, total_edits: usize) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        new_name = %new_name,
        files,
        total_edits,
        "rename"
    );
}

pub(super) async fn prepare_rename_impl(
    backend: &Backend,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let uri = params.text_document.uri;
    let pos_ls = params.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    if DocumentFormat::from_uri(&uri).is_none() {
        return Ok(None);
    }

    let domain = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::rename::prepare(text, state.tree.as_ref(), byte)
    });
    let Some(Some(domain)) = domain else {
        log_prepare_rename_not_renameable(&uri);
        return Ok(None);
    };

    let range = backend
        .store
        .docs_iter_for_uri(&uri, |state| {
            byte_range_to_ls(&state.doc, &domain.byte_range)
        })
        .unwrap_or(Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        });

    log_prepare_rename(&uri, &domain.placeholder);

    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
        range,
        placeholder: domain.placeholder,
    }))
}

pub(super) async fn rename_impl(
    backend: &Backend,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    let uri = params.text_document_position.text_document.uri;
    let pos_ls = params.text_document_position.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let new_name = params.new_name;
    if DocumentFormat::from_uri(&uri).is_none() {
        return Ok(None);
    }

    let domain = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::rename::compute(
            text,
            state.tree.as_ref(),
            &uri,
            backend.store.as_ref(),
            Some(backend.workspace_tables.as_ref()),
            byte,
            &new_name,
        )
    });
    let Some(Some(domain)) = domain else {
        return Ok(None);
    };

    log_rename(
        &uri,
        &new_name,
        domain.edits.len(),
        domain.edits.values().map(Vec::len).sum::<usize>(),
    );

    Ok(
        lowered_rename_changes(domain.edits, backend).map(|changes| WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }),
    )
}

pub(super) fn lowered_rename_changes(
    edits: BTreeMap<tower_lsp_server::ls_types::Uri, Vec<crate::rename::DomainTextEdit>>,
    backend: &Backend,
) -> Option<HashMap<tower_lsp_server::ls_types::Uri, Vec<tower_lsp_server::ls_types::TextEdit>>> {
    let mut changes = HashMap::new();
    for (target_uri, domain_edits) in edits {
        let Some(text_edits) = domain_edits_to_lsp(&target_uri, &domain_edits, backend) else {
            continue;
        };
        changes.insert(target_uri, text_edits);
    }

    (!changes.is_empty()).then_some(changes)
}
