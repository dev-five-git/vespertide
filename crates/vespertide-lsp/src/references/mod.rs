//! Find-references — pure domain layer.
//!
//! Two reference kinds are recognized:
//! * **Table reference** — cursor on a top-level `name` value, on a
//!   `foreign_key.ref_table` value, or on any other identifier that
//!   resolves to a workspace table. Returns every usage of that table
//!   name across the workspace (declaration + every `ref_table`).
//! * **Column reference** — cursor on a column object's `name` value or
//!   on a string element of a `ref_columns` array. Returns every
//!   `ref_columns` entry that targets *this* column on *its* table.
//!
//! Cross-file resolution honours both open documents (via [`DocumentStore`])
//! and disk-only models (via [`WorkspaceTables`]) so the result includes
//! files the user has not yet opened.

mod resolver;
mod search;

use std::ops::Range;

use tower_lsp_server::ls_types::Uri;

use crate::parser::DocumentFormat;
use crate::store::DocumentStore;
use crate::workspace_index::WorkspaceIndex;
use crate::workspace_tables::WorkspaceTables;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainReference {
    pub uri: Uri,
    pub byte_range: Range<usize>,
}

/// Symbol the cursor resolved to. Exposed so callers (e.g. rename) can
/// reuse the resolution logic without rewalking the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceSymbol {
    /// Reference target is a table.
    Table { name: String },
    /// Reference target is a column qualified by its owning table.
    Column { table: String, column: String },
}

/// Compute references for the symbol at `byte_offset`.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "references compute threads document parse state, workspace stores, cursor, and declaration policy; ReferenceContext is a deferred refactor"
)]
pub fn compute(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    current_uri: &Uri,
    index: &WorkspaceIndex,
    docs: &DocumentStore,
    disk_tables: Option<&WorkspaceTables>,
    byte_offset: usize,
    include_declaration: bool,
) -> Vec<DomainReference> {
    let _ = format;
    let Some(symbol) = resolve_symbol(source, tree, current_uri, byte_offset) else {
        return Vec::new();
    };

    search::find_all(
        &symbol,
        current_uri,
        source,
        tree,
        index,
        docs,
        disk_tables,
        include_declaration,
    )
}

/// Public helper so rename can resolve the symbol the cursor sits on.
#[must_use]
pub fn resolve_symbol(
    source: &str,
    tree: Option<&tree_sitter::Tree>,
    current_uri: &Uri,
    byte_offset: usize,
) -> Option<ReferenceSymbol> {
    resolver::resolve(source, tree, current_uri, byte_offset)
}
