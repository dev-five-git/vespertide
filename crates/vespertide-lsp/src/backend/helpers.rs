//! Standalone helpers extracted from `backend::mod` to keep the
//! per-file line count under the workspace's 1000-line policy. None
//! of these functions touch private internals of [`Backend`] — they
//! either take primitives or accept a `&Backend` and use its
//! `pub(super)` surface.

use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind as LspCompletionItemKind, CompletionTextEdit, Diagnostic,
    DiagnosticSeverity, InsertTextFormat, Location, Range, SymbolInformation,
    SymbolKind as LspSymbolKind, TextEdit, Uri,
};

use super::Backend;

#[derive(Default)]
pub(super) struct DiagnosticSeverityCounts {
    pub errors: usize,
    pub warnings: usize,
}

pub(super) fn diagnostic_severity_counts(diagnostics: &[Diagnostic]) -> DiagnosticSeverityCounts {
    let mut counts = DiagnosticSeverityCounts::default();
    for diag in diagnostics {
        match diag.severity {
            Some(DiagnosticSeverity::ERROR) => counts.errors += 1,
            Some(DiagnosticSeverity::WARNING) => counts.warnings += 1,
            _ => {}
        }
    }
    counts
}

/// Translate a [`crate::completion::DomainCompletion`] into the LSP wire
/// shape. When the domain layer supplies a byte range to replace, we lower
/// it to a `TextEdit` so the client wipes the existing string (quotes and
/// all) before inserting the snippet — that is what makes typing `varchar`
/// inside `""` collapse the quotes and unfold into a `{...}` object literal.
pub(super) fn domain_to_lsp(
    item: crate::completion::DomainCompletion,
    doc: &lsp_textdocument::FullTextDocument,
) -> CompletionItem {
    let kind = Some(match item.kind {
        crate::completion::CompletionItemKind::Value => LspCompletionItemKind::VALUE,
        crate::completion::CompletionItemKind::Property => LspCompletionItemKind::PROPERTY,
        crate::completion::CompletionItemKind::Reference => LspCompletionItemKind::REFERENCE,
        crate::completion::CompletionItemKind::Snippet => LspCompletionItemKind::SNIPPET,
    });

    let text_edit = item.replace_range_bytes.as_ref().map(|range| {
        let start = byte_to_ls_position(doc, range.start);
        let end = byte_to_ls_position(doc, range.end);
        let new_text = item
            .insert_text
            .clone()
            .unwrap_or_else(|| item.label.clone());
        CompletionTextEdit::Edit(TextEdit {
            range: Range { start, end },
            new_text,
        })
    });

    let insert_text_format = item.insert_text.as_ref().map(|_| InsertTextFormat::SNIPPET);
    // Per LSP spec: when text_edit is set, the client ignores insert_text.
    // Suppress it so the two never disagree.
    let insert_text = if text_edit.is_some() {
        None
    } else {
        item.insert_text
    };
    let sort_text = Some(format!("{:03}{}", item.sort_priority, item.label));

    CompletionItem {
        label: item.label,
        kind,
        detail: item.detail,
        text_edit,
        insert_text_format,
        insert_text,
        sort_text,
        ..CompletionItem::default()
    }
}

pub(super) fn byte_to_ls_position(
    doc: &lsp_textdocument::FullTextDocument,
    byte_offset: usize,
) -> tower_lsp_server::ls_types::Position {
    crate::position::lsp_to_ls_position(crate::position::byte_to_lsp_position(doc, byte_offset))
}

/// Best-effort filesystem path normalization for workspace dedup.
///
/// 1. `std::fs::canonicalize` when the file exists — that is the most
///    reliable cross-tool match (resolves symlinks + UNC + casing).
/// 2. Fallback to forward-slash + lowercase rewrite so Windows files that
///    differ only in drive-letter case still compare equal.
pub(super) fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let lossy = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        std::path::PathBuf::from(lossy.to_lowercase())
    } else {
        std::path::PathBuf::from(lossy)
    }
}

/// Lower a domain symbol into LSP `SymbolInformation`, resolving the byte
/// range via either the open document or the on-disk file.
#[expect(
    deprecated,
    reason = "workspace/symbol still lowers to deprecated SymbolInformation for downlevel LSP client compatibility"
)]
pub(super) fn symbol_to_lsp(
    symbol: &crate::symbols::DomainSymbol,
    backend: &Backend,
) -> Option<SymbolInformation> {
    let range = backend.store.docs_iter_for_uri(&symbol.uri, |state| Range {
        start: byte_to_ls_position(&state.doc, symbol.byte_range.start),
        end: byte_to_ls_position(&state.doc, symbol.byte_range.end),
    });
    let range = if let Some(r) = range {
        r
    } else {
        // Disk-only file — read it once to get a UTF-16-aware doc.
        let path = crate::position::uri_to_path(&symbol.uri)?;
        let text = std::fs::read_to_string(&path).ok()?;
        let language_id = match path.extension().and_then(|e| e.to_str()) {
            Some("yaml" | "yml") => "yaml",
            _ => "json",
        };
        let doc = lsp_textdocument::FullTextDocument::new(language_id.to_string(), 1, text);
        Range {
            start: byte_to_ls_position(&doc, symbol.byte_range.start),
            end: byte_to_ls_position(&doc, symbol.byte_range.end),
        }
    };

    Some(SymbolInformation {
        name: symbol.name.clone(),
        kind: match symbol.kind {
            crate::symbols::SymbolKind::Table => LspSymbolKind::CLASS,
            crate::symbols::SymbolKind::Column => LspSymbolKind::FIELD,
        },
        location: Location {
            uri: symbol.uri.clone(),
            range,
        },
        container_name: symbol.container.clone(),
        tags: None,
        deprecated: None,
    })
}

/// Convert a list of [`crate::rename::DomainTextEdit`] into LSP `TextEdit`s
/// for `target_uri`. Mirrors the open-vs-disk fallback used by references.
pub(super) fn domain_edits_to_lsp(
    target_uri: &Uri,
    domain_edits: &[crate::rename::DomainTextEdit],
    backend: &Backend,
) -> Option<Vec<TextEdit>> {
    let to_lsp = |doc: &lsp_textdocument::FullTextDocument| {
        domain_edits
            .iter()
            .map(|edit| TextEdit {
                range: Range {
                    start: byte_to_ls_position(doc, edit.byte_range.start),
                    end: byte_to_ls_position(doc, edit.byte_range.end),
                },
                new_text: edit.new_text.clone(),
            })
            .collect()
    };

    if let Some(edits) = backend
        .store
        .docs_iter_for_uri(target_uri, |state| to_lsp(&state.doc))
    {
        return Some(edits);
    }

    let path = crate::position::uri_to_path(target_uri)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let language_id = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml" | "yml") => "yaml",
        _ => "json",
    };
    let doc = lsp_textdocument::FullTextDocument::new(language_id.to_string(), 1, text);
    Some(to_lsp(&doc))
}

/// Convert a [`crate::references::DomainReference`] into an LSP [`Location`].
///
/// When the target URI is an open document we use its `FullTextDocument`
/// for accurate UTF-16 offset conversion. For disk-only files we read the
/// source and build a transient document. Returns `None` only when both
/// fail.
pub(super) fn domain_reference_to_location(
    reference: &crate::references::DomainReference,
    backend: &Backend,
) -> Option<Location> {
    if let Some(range) = backend
        .store
        .docs_iter_for_uri(&reference.uri, |state| Range {
            start: byte_to_ls_position(&state.doc, reference.byte_range.start),
            end: byte_to_ls_position(&state.doc, reference.byte_range.end),
        })
    {
        return Some(Location {
            uri: reference.uri.clone(),
            range,
        });
    }

    // Disk-only file — read source on demand.
    let path = crate::position::uri_to_path(&reference.uri)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let language_id = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml" | "yml") => "yaml",
        _ => "json",
    };
    let doc = lsp_textdocument::FullTextDocument::new(language_id.to_string(), 1, text);
    Some(Location {
        uri: reference.uri.clone(),
        range: Range {
            start: byte_to_ls_position(&doc, reference.byte_range.start),
            end: byte_to_ls_position(&doc, reference.byte_range.end),
        },
    })
}
