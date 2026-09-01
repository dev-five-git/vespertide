//! Body of the file-local LSP handlers (`document_symbol`,
//! `folding_range`, `document_highlight`, `selection_range`), lifted
//! out of `backend::mod` to keep that file under the workspace line
//! policy.

#![expect(
    clippy::unused_async,
    reason = "tower-lsp-server LanguageServer file-feature handlers must stay awaitable async fns even when bodies are synchronous"
)]
#![expect(
    deprecated,
    reason = "documentSymbol still emits the deprecated DocumentSymbol::deprecated field for downlevel client compatibility"
)]

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, DocumentSymbol,
    DocumentSymbolParams, DocumentSymbolResponse, FoldingRange, FoldingRangeParams, SelectionRange,
    SelectionRangeParams, SymbolKind as LspSymbolKind,
};

use super::Backend;
use super::helpers::{byte_range_to_ls, byte_to_ls_position, non_empty};

pub(super) async fn document_symbol_impl(
    backend: &Backend,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let uri = params.text_document.uri;
    let symbols = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let domain = crate::file_features::compute_document_symbols(text, state.tree.as_ref());
        domain
            .into_iter()
            .map(|s| domain_doc_sym_to_lsp(s, &state.doc))
            .collect::<Vec<_>>()
    });
    Ok(symbols.map(DocumentSymbolResponse::Nested))
}

pub(super) async fn folding_range_impl(
    backend: &Backend,
    params: FoldingRangeParams,
) -> Result<Option<Vec<FoldingRange>>> {
    let uri = params.text_document.uri;
    let folds = backend.store.docs_iter_for_uri(&uri, |state| {
        let domain = crate::file_features::compute_folding_ranges(state.tree.as_ref());
        domain
            .into_iter()
            .filter_map(|f| {
                let start = byte_to_ls_position(&state.doc, f.byte_range.start);
                let end = byte_to_ls_position(&state.doc, f.byte_range.end);
                if start.line == end.line {
                    return None;
                }
                Some(FoldingRange {
                    start_line: start.line,
                    start_character: None,
                    end_line: end.line,
                    end_character: None,
                    kind: None,
                    collapsed_text: None,
                })
            })
            .collect::<Vec<_>>()
    });
    Ok(non_empty(folds.unwrap_or_default()))
}

pub(super) async fn document_highlight_impl(
    backend: &Backend,
    params: DocumentHighlightParams,
) -> Result<Option<Vec<DocumentHighlight>>> {
    let uri = params.text_document_position_params.text_document.uri;
    let pos_ls = params.text_document_position_params.position;
    let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
    let hits = backend.store.docs_iter_for_uri(&uri, |state| {
        let text = state.text();
        let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
        crate::file_features::compute_document_highlight(text, state.tree.as_ref(), byte)
            .into_iter()
            .map(|h| DocumentHighlight {
                range: byte_range_to_ls(&state.doc, &h.byte_range),
                kind: Some(match h.kind {
                    crate::file_features::DomainDocumentHighlightKind::Read => {
                        DocumentHighlightKind::READ
                    }
                    crate::file_features::DomainDocumentHighlightKind::Reference => {
                        DocumentHighlightKind::TEXT
                    }
                }),
            })
            .collect::<Vec<_>>()
    });
    Ok(non_empty(hits.unwrap_or_default()))
}

pub(super) async fn selection_range_impl(
    backend: &Backend,
    params: SelectionRangeParams,
) -> Result<Option<Vec<SelectionRange>>> {
    let uri = params.text_document.uri;
    let positions = params.positions;
    let ranges = backend.store.docs_iter_for_uri(&uri, |state| {
        positions
            .into_iter()
            .map(|pos_ls| {
                let pos_lsp = crate::position::ls_to_lsp_position(pos_ls);
                let byte = crate::position::lsp_position_to_byte(&state.doc, pos_lsp);
                let chain =
                    crate::file_features::compute_selection_ranges(state.tree.as_ref(), byte);
                let mut acc: Option<Box<SelectionRange>> = None;
                for entry in chain.into_iter().rev() {
                    let lsp_range = byte_range_to_ls(&state.doc, &entry.byte_range);
                    acc = Some(Box::new(SelectionRange {
                        range: lsp_range,
                        parent: acc,
                    }));
                }
                match acc {
                    Some(boxed) => *boxed,
                    None => SelectionRange {
                        range: byte_range_to_ls(&state.doc, &(byte..byte)),
                        parent: None,
                    },
                }
            })
            .collect::<Vec<_>>()
    });
    Ok(ranges)
}

fn domain_doc_sym_to_lsp(
    sym: crate::file_features::DomainDocumentSymbol,
    doc: &lsp_textdocument::FullTextDocument,
) -> DocumentSymbol {
    let range = byte_range_to_ls(doc, &sym.byte_range);
    let selection_range = byte_range_to_ls(doc, &sym.selection_byte_range);
    let kind = match sym.kind {
        crate::file_features::DomainDocumentSymbolKind::Table => LspSymbolKind::CLASS,
        crate::file_features::DomainDocumentSymbolKind::Column => LspSymbolKind::FIELD,
    };
    let children = if sym.children.is_empty() {
        None
    } else {
        Some(
            sym.children
                .into_iter()
                .map(|c| domain_doc_sym_to_lsp(c, doc))
                .collect(),
        )
    };
    DocumentSymbol {
        name: sym.name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}
