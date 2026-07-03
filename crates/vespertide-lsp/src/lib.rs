//! `vespertide-lsp`: Language Server for Vespertide schema files.
//!
//! Provides diagnostics, hover, cross-file go-to-definition, completion,
//! and drift detection (model <-> migration consistency) for the
//! Vespertide JSON / YAML model and migration formats.
//!
//! Wave 2 adds the data layer: tree-sitter parsing ([`ParserPool`]),
//! per-document state ([`DocumentState`]), and a concurrent
//! [`DocumentStore`]. W2-T2 wires the `did_open` / `did_change` /
//! `did_close` notification handlers and adds UTF-16 ↔ byte offset
//! conversions; W2-T3 introduces [`WorkspaceIndex`], a cross-file
//! `table_name → Uri` map maintained by walking each document's
//! tree-sitter parse.

mod backend;
pub(crate) mod cache;
mod check_expr_locate;
mod check_expr_range;
mod code_actions;
mod completion;
mod definition;
pub mod diagnostics;
mod document;
mod drift;
mod file_features;
mod formatting;
mod hover;
mod inlay_hints;
pub mod logging;
mod parser;
mod position;
mod references;
mod rename;
pub mod semantic_tokens;
mod store;
mod symbols;
#[cfg(test)]
pub(crate) mod test_support;
mod text_util;
pub mod watched_files;
mod workspace_index;
pub mod workspace_tables;

pub use backend::Backend;
pub use code_actions::{
    CodeActionKind as DomainCodeActionKind, DomainCodeAction, compute as compute_code_actions,
};
pub use completion::{
    CompletionItemKind, DomainCompletion, compute as compute_completion,
    compute_with_workspace_tables as compute_completion_with_workspace_tables,
};
pub use definition::{DomainLocation, compute as compute_definition};
pub use diagnostics::{
    DomainDiagnostic, Severity, compute as compute_diagnostics,
    compute_shared as compute_diagnostics_shared,
    compute_workspace as compute_workspace_diagnostics,
};
pub use document::DocumentState;
pub use drift::{
    DomainDrift, DriftCache, DriftKind, compute as compute_drift,
    compute_with_cache as compute_drift_with_cache,
};
pub use file_features::{
    DomainDocumentHighlight, DomainDocumentHighlightKind, DomainDocumentSymbol,
    DomainDocumentSymbolKind, DomainFoldingRange, DomainSelectionRange, compute_document_highlight,
    compute_document_symbols, compute_folding_ranges, compute_selection_ranges,
};
pub use formatting::format_text;
pub use hover::{DomainHover, compute as compute_hover};
pub use inlay_hints::{DomainInlayHint, compute as compute_inlay_hints};
pub use parser::{DocumentFormat, ParserPool};
pub use position::{
    byte_to_lsp_position, ls_to_lsp_position, ls_to_lsp_range, lsp_position_to_byte,
    lsp_to_ls_position, uri_to_path,
};
pub use references::{
    DomainReference, ReferenceSymbol, compute as compute_references,
    resolve_symbol as resolve_reference_symbol,
};
pub use rename::{
    DomainPrepareRename, DomainRename, DomainTextEdit, compute as compute_rename,
    prepare as prepare_rename,
};
pub use semantic_tokens::classify_shared as classify_semantic_tokens_shared;
pub use store::DocumentStore;
pub use symbols::{
    DomainSymbol, SymbolKind as DomainSymbolKind, compute as compute_workspace_symbols,
    compute_shared as compute_workspace_symbols_shared,
};
pub use workspace_index::{TableLocation, WorkspaceIndex};
pub use workspace_tables::WorkspaceTables;
