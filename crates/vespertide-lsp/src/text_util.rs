//! Small text helpers shared across LSP features.

/// Strip surrounding JSON/SQL quote characters from a scalar's raw text.
/// Greedily trims leading/trailing double (`"`) then single (`'`) quotes
/// after trimming whitespace — the canonical form used across the LSP's
/// JSON/YAML scalar handling (a JSON `"..."` wrapper plus an inner SQL
/// `'...'` literal both peel cleanly).
#[must_use]
pub(crate) fn strip_quotes(s: &str) -> &str {
    s.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('\'')
        .trim_end_matches('\'')
}

/// Peel ONLY the outer JSON `"..."` wrapper from a scalar's raw text after
/// trimming whitespace. Leaves any inner SQL `'...'` literal untouched —
/// required by the code-action `default → enum` path which peels the JSON
/// quote first, then runs an explicit `.strip_prefix('\'').strip_suffix('\'')`
/// pair to recognise the SQL literal inside.
///
/// Canonical replacement for the three byte-equivalent local `strip_quotes`
/// helpers previously open-coded in `code_actions`, `workspace_index`, and
/// `diagnostics::locator` (the locator version was a matched-pair `"..."` OR
/// `'...'` peeler, but every actual JSON key/value the locator consumes is
/// well-formed JSON so the greedy `"`-only form is observationally
/// identical there). Use [`strip_quotes`] when you want both flavours
/// stripped (the JSON/YAML scalar reading path).
#[must_use]
pub(crate) fn strip_json_quotes(s: &str) -> &str {
    s.trim().trim_start_matches('"').trim_end_matches('"')
}

/// UTF-8 slice of a tree-sitter node's byte range. Returns `None` when the
/// slice is not valid UTF-8 — defensive only; the LSP parsers produce valid
/// UTF-8 spans on every source we feed them. Single source of truth for the
/// `std::str::from_utf8(&source[node.byte_range()]).ok()` chain that used to
/// be open-coded across `diagnostics/locator`, `code_actions`, and friends.
#[must_use]
pub(crate) fn node_text<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    std::str::from_utf8(&source[node.byte_range()]).ok()
}
