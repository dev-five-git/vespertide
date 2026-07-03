//! Source text + tree-sitter tree retrieval for drift anchoring.
//!
//! When the document is open in the editor, the in-memory copy and cached
//! tree are reused. Otherwise the file is parsed on-demand from disk via
//! the shared parser pool.

use tower_lsp_server::ls_types::Uri;
use tree_sitter::Tree;

use crate::parser::{DocumentFormat, ParserPool};
use crate::store::DocumentStore;

pub(super) fn source_and_tree(
    uri: &Uri,
    docs: &DocumentStore,
    parser_pool: &ParserPool,
) -> Option<(String, Option<Tree>)> {
    docs.with_doc(uri, |source, tree| (source.to_string(), tree.cloned()))
        .or_else(|| source_and_tree_from_disk(uri, parser_pool))
}

fn source_and_tree_from_disk(
    uri: &Uri,
    parser_pool: &ParserPool,
) -> Option<(String, Option<Tree>)> {
    let path = crate::position::uri_to_path(uri)?;
    let source = std::fs::read_to_string(path).ok()?;
    let tree = DocumentFormat::from_uri(uri).and_then(|format| parser_pool.parse(&source, format));
    Some((source, tree))
}
