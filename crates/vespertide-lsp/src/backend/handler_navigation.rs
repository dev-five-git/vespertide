//! Body of the navigation / point-at-position LSP handlers
//! (`completion`, `hover`, `goto_definition`, `references`,
//! `code_action`, `inlay_hint`, `symbol`), lifted out of `backend::mod`
//! to keep that file under the workspace's 1000-line per-file policy.
//!
//! Each `*_impl` function takes `&Backend` so it can read shared state
//! (store, index, workspace tables) via the `pub` fields without
//! depending on private internals. The trait wrappers in `backend::mod`
//! delegate verbatim.
//!
//! `async` is preserved on every helper — the LSP trait expects an
//! `async fn`, and these wrappers must be `.await`-able from the trait
//! impl block in `mod.rs`. Several bodies (e.g. `inlay_hint_impl`,
//! `symbol_impl`) don't `.await` and so trip clippy's `unused_async`,
//! mirroring the existing `handler_rename.rs` / `handler_file_features.rs`
//! pattern.
#![expect(
    clippy::unused_async,
    reason = "tower-lsp-server LanguageServer navigation handlers must stay awaitable async fns even when bodies are synchronous"
)]

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind as LspCodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionResponse, CompletionParams, CompletionResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InlayHint,
    InlayHintKind as LspInlayHintKind, InlayHintLabel, InlayHintParams, Location, MarkupContent,
    MarkupKind, Position, Range, ReferenceParams, SymbolInformation, Uri, WorkspaceEdit,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use super::Backend;
use super::helpers::{
    byte_to_ls_position, domain_edits_to_lsp, domain_reference_to_location, domain_to_lsp,
    non_empty, symbol_to_lsp,
};
use crate::parser::DocumentFormat;

#[cfg(not(tarpaulin_include))]
fn log_completion_unsupported_format(uri: &Uri) {
    tracing::debug!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        "completion: unsupported document format"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_completion(uri: &Uri, line: u32, character: u32, count: usize) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        line,
        character,
        items = count,
        "completion"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_goto_definition_no_target(uri: &Uri, line: u32, character: u32) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        line,
        character,
        "goto_definition: no target"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_goto_definition_resolved(uri: &Uri, target_uri: &Uri, line: u32, character: u32) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        from_uri = %uri.as_str(),
        target_uri = %target_uri.as_str(),
        line,
        character,
        "goto_definition: resolved"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_references(uri: &Uri, line: u32, character: u32, include_declaration: bool, count: usize) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        line,
        character,
        include_declaration,
        count,
        "references"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_code_action(uri: &Uri, actions: usize) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        actions,
        "code_action"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_inlay_hint(uri: &Uri, count: usize) {
    tracing::debug!(
        target: "vespertide_lsp::handler",
        uri = %uri.as_str(),
        count,
        "inlay_hint"
    );
}

#[cfg(not(tarpaulin_include))]
fn log_workspace_symbol(query: &str, results: usize) {
    tracing::info!(
        target: "vespertide_lsp::handler",
        query = %query,
        results,
        "workspace symbol"
    );
}

pub(super) async fn completion_impl(
    backend: &Backend,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let uri = &params.text_document_position.text_document.uri;
    let pos_ls = params.text_document_position.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let Some(format) = DocumentFormat::from_uri(uri) else {
        log_completion_unsupported_format(uri);
        return Ok(None);
    };

    let items = backend.store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::completion::compute_with_workspace_tables(
            text,
            format,
            state.tree.as_ref(),
            backend.index.as_ref(),
            backend.store.as_ref(),
            backend.workspace_tables.as_ref(),
            byte,
        )
        .into_iter()
        .map(|item| domain_to_lsp(item, &state.doc))
        .collect::<Vec<_>>()
    });

    let count = items.as_ref().map_or(0, Vec::len);
    log_completion(uri, pos_lsp.line, pos_lsp.character, count);

    Ok(items.map(CompletionResponse::Array))
}

pub(super) async fn hover_impl(backend: &Backend, params: HoverParams) -> Result<Option<Hover>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let pos_ls = params.text_document_position_params.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    if DocumentFormat::from_uri(uri).is_none() {
        return Ok(None);
    }

    let result = backend.store.docs_iter_for_uri(uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        let domain = crate::hover::compute_with_workspace_tables(
            text,
            state.tree.as_ref(),
            backend.index.as_ref(),
            backend.store.as_ref(),
            Some(backend.workspace_tables.as_ref()),
            byte,
        )?;
        let start = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.start);
        let end = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.end);
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: domain.markdown,
            }),
            range: Some(Range {
                start: Position {
                    line: start.line,
                    character: start.character,
                },
                end: Position {
                    line: end.line,
                    character: end.character,
                },
            }),
        })
    });
    Ok(result.flatten())
}

pub(super) async fn goto_definition_impl(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = params.text_document_position_params.text_document.uri;
    let pos_ls = params.text_document_position_params.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    if DocumentFormat::from_uri(&uri).is_none() {
        return Ok(None);
    }

    let domain = backend
        .store
        .docs_iter_for_uri(&uri, |state| {
            let text = state.text();
            let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
            crate::definition::compute_with_workspace_tables(
                text,
                state.tree.as_ref(),
                backend.index.as_ref(),
                backend.store.as_ref(),
                Some(backend.workspace_tables.as_ref()),
                byte,
            )
        })
        .flatten();

    let Some(domain) = domain else {
        log_goto_definition_no_target(&uri, pos_lsp.line, pos_lsp.character);
        return Ok(None);
    };
    log_goto_definition_resolved(&uri, &domain.uri, pos_lsp.line, pos_lsp.character);

    let target_range = backend
        .store
        .docs_iter_for_uri(&domain.uri, |state| {
            let start = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.start);
            let end = crate::position::byte_to_lsp_position(&state.doc, domain.byte_range.end);
            Range {
                start: Position {
                    line: start.line,
                    character: start.character,
                },
                end: Position {
                    line: end.line,
                    character: end.character,
                },
            }
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

    Ok(Some(GotoDefinitionResponse::Scalar(Location {
        uri: domain.uri,
        range: target_range,
    })))
}

pub(super) async fn references_impl(
    backend: &Backend,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let uri = params.text_document_position.text_document.uri;
    let pos_ls = params.text_document_position.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let include_declaration = params.context.include_declaration;
    if DocumentFormat::from_uri(&uri).is_none() {
        return Ok(None);
    }

    let domain_refs = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::references::compute(
            text,
            state.tree.as_ref(),
            &uri,
            backend.store.as_ref(),
            Some(backend.workspace_tables.as_ref()),
            byte,
            include_declaration,
        )
    });
    let Some(domain_refs) = domain_refs else {
        return Ok(None);
    };

    log_references(
        &uri,
        pos_lsp.line,
        pos_lsp.character,
        include_declaration,
        domain_refs.len(),
    );

    let locations = domain_refs
        .into_iter()
        .filter_map(|reference| domain_reference_to_location(&reference, backend))
        .collect::<Vec<_>>();

    Ok(non_empty(locations))
}

pub(super) async fn code_action_impl(
    backend: &Backend,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let uri = params.text_document.uri;
    let range_ls = params.range;
    let range_lsp = crate::position::ls_to_lsp_range(range_ls);
    let Some(format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let domain_actions = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let start = crate::position::lsp_position_to_byte(&state.doc, range_lsp.start);
        let end = crate::position::lsp_position_to_byte(&state.doc, range_lsp.end);
        crate::code_actions::compute(text, format, state.tree.as_ref(), start..end)
    });
    let Some(domain_actions) = domain_actions else {
        return Ok(None);
    };

    log_code_action(&uri, domain_actions.len());

    let actions: Vec<CodeActionOrCommand> = domain_actions
        .into_iter()
        .filter_map(|action| {
            let text_edits = domain_edits_to_lsp(&uri, &action.edits, backend)?;
            let mut changes = std::collections::HashMap::new();
            changes.insert(uri.clone(), text_edits);
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: action.title,
                kind: Some(LspCodeActionKind::REFACTOR),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                }),
                ..CodeAction::default()
            }))
        })
        .collect();

    Ok(non_empty(actions))
}

pub(super) async fn inlay_hint_impl(
    backend: &Backend,
    params: InlayHintParams,
) -> Result<Option<Vec<InlayHint>>> {
    let uri = params.text_document.uri;
    let range_ls = params.range;
    let range_lsp = crate::position::ls_to_lsp_range(range_ls);
    let Some(_format) = DocumentFormat::from_uri(&uri) else {
        return Ok(None);
    };

    let hints = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let start = crate::position::lsp_position_to_byte(&state.doc, range_lsp.start);
        let end = crate::position::lsp_position_to_byte(&state.doc, range_lsp.end);
        let domain = crate::inlay_hints::compute(text, state.tree.as_ref(), start..end);
        domain
            .into_iter()
            .map(|hint| InlayHint {
                position: byte_to_ls_position(&state.doc, hint.byte_offset),
                label: InlayHintLabel::String(hint.label),
                kind: Some(LspInlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(false),
                padding_right: Some(false),
                data: None,
            })
            .collect::<Vec<_>>()
    });

    let Some(hints) = hints else {
        return Ok(None);
    };

    log_inlay_hint(&uri, hints.len());

    Ok(non_empty(hints))
}

pub(super) async fn symbol_impl(
    backend: &Backend,
    params: WorkspaceSymbolParams,
) -> Result<Option<WorkspaceSymbolResponse>> {
    let query = params.query;
    let domain = crate::symbols::compute_shared(
        &query,
        backend.store.as_ref(),
        Some(backend.workspace_tables.as_ref()),
    );

    log_workspace_symbol(&query, domain.len());

    let lsp_symbols: Vec<SymbolInformation> = domain
        .iter()
        .filter_map(|sym| symbol_to_lsp(sym, backend))
        .collect();
    Ok(non_empty(lsp_symbols).map(WorkspaceSymbolResponse::Flat))
}
