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
        let new_text = item
            .insert_text
            .clone()
            .unwrap_or_else(|| item.label.clone());
        CompletionTextEdit::Edit(TextEdit {
            range: byte_range_to_ls(doc, range),
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

/// Lower a byte-offset range to an LSP [`Range`] via UTF-16-aware position
/// conversion. This is the single place the start/end pair lowering lives —
/// every handler that turns a domain `byte_range` into a wire `Range` goes
/// through here.
pub(super) fn byte_range_to_ls(
    doc: &lsp_textdocument::FullTextDocument,
    range: &std::ops::Range<usize>,
) -> Range {
    Range {
        start: byte_to_ls_position(doc, range.start),
        end: byte_to_ls_position(doc, range.end),
    }
}

/// `None` when the collected result set is empty, so handlers can answer
/// `Ok(None)` instead of shipping an empty payload over the wire.
pub(super) fn non_empty<T>(v: Vec<T>) -> Option<Vec<T>> {
    if v.is_empty() { None } else { Some(v) }
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

/// Read a disk-only file into a transient UTF-16-aware document, guessing
/// the language id from the file extension. Shared fallback for symbol,
/// reference, and rename lowering when the target URI is not an open
/// document. Returns `None` when the URI has no path or the read fails.
fn disk_document(uri: &Uri) -> Option<lsp_textdocument::FullTextDocument> {
    let path = crate::position::uri_to_path(uri)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let language_id = match path.extension().and_then(|e| e.to_str()) {
        Some("yaml" | "yml") => "yaml",
        _ => "json",
    };
    Some(lsp_textdocument::FullTextDocument::new(
        language_id.to_string(),
        1,
        text,
    ))
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
    let range = backend.store.docs_iter_for_uri(&symbol.uri, |state| {
        byte_range_to_ls(&state.doc, &symbol.byte_range)
    });
    let range = if let Some(r) = range {
        r
    } else {
        // Disk-only file — read it once to get a UTF-16-aware doc.
        let doc = disk_document(&symbol.uri)?;
        byte_range_to_ls(&doc, &symbol.byte_range)
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
                range: byte_range_to_ls(doc, &edit.byte_range),
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

    let doc = disk_document(target_uri)?;
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
    if let Some(range) = backend.store.docs_iter_for_uri(&reference.uri, |state| {
        byte_range_to_ls(&state.doc, &reference.byte_range)
    }) {
        return Some(Location {
            uri: reference.uri.clone(),
            range,
        });
    }

    // Disk-only file — read source on demand.
    let doc = disk_document(&reference.uri)?;
    Some(Location {
        uri: reference.uri.clone(),
        range: byte_range_to_ls(&doc, &reference.byte_range),
    })
}
