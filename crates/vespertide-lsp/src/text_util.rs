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
