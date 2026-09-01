//! SQL string-literal escaping helpers, shared across crates.
//!
//! Mirrors how production SQL emitters handle single-quote escape in
//! string literals. Centralizing the helper means future hardening (NUL
//! byte filtering, backslash escapes for MySQL `ANSI_QUOTES`, …) lands in
//! one place instead of being re-applied across 5+ callsites in
//! `vespertide-cli`, `vespertide-core`, and `vespertide-query`.

use std::borrow::Cow;

/// Escape SQL single quotes by doubling them — the canonical, portable
/// way to embed a string inside a `'...'` literal across Postgres /
/// `MySQL` / `SQLite`. Returns a borrowed slice when no escape is required
/// (zero allocation), otherwise an owned `String`.
///
/// ```rust
/// use std::borrow::Cow;
/// use vespertide_core::escape_sql_string_literal;
///
/// assert!(matches!(escape_sql_string_literal("hello"), Cow::Borrowed(_)));
/// assert_eq!(escape_sql_string_literal("O'Brien"), "O''Brien");
/// assert_eq!(escape_sql_string_literal("'leading"), "''leading");
/// assert_eq!(escape_sql_string_literal(""), "");
/// ```
#[must_use]
pub fn escape_sql_string_literal(s: &str) -> Cow<'_, str> {
    if s.contains('\'') {
        Cow::Owned(s.replace('\'', "''"))
    } else {
        Cow::Borrowed(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_borrows() {
        let out = escape_sql_string_literal("");
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, "");
    }

    #[test]
    fn clean_string_borrows() {
        let out = escape_sql_string_literal("hello world");
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, "hello world");
    }

    #[test]
    fn single_quote_in_middle_escapes() {
        let out = escape_sql_string_literal("O'Brien");
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out, "O''Brien");
    }

    #[test]
    fn leading_quote_escapes() {
        let out = escape_sql_string_literal("'leading");
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out, "''leading");
    }

    #[test]
    fn trailing_quote_escapes() {
        let out = escape_sql_string_literal("trailing'");
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out, "trailing''");
    }

    #[test]
    fn only_quote_escapes() {
        let out = escape_sql_string_literal("'");
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out, "''");
    }

    #[test]
    fn multiple_quotes_escape_each() {
        let out = escape_sql_string_literal("a'b'c'");
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out, "a''b''c''");
    }

    #[test]
    fn consecutive_quotes_escape_each() {
        let out = escape_sql_string_literal("''");
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out, "''''");
    }

    #[test]
    fn unicode_preserved_when_borrowed() {
        let out = escape_sql_string_literal("héllo 世界");
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, "héllo 世界");
    }

    #[test]
    fn unicode_preserved_when_owned() {
        let out = escape_sql_string_literal("héllo'世界");
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(out, "héllo''世界");
    }

    #[test]
    fn matches_naive_replace() {
        // Equivalence oracle: helper output must equal raw .replace()
        for s in [
            "",
            "x",
            "'",
            "''",
            "a'",
            "'a",
            "a'b",
            "O'Brien's",
            "no quotes here",
            "héllo 世界",
        ] {
            assert_eq!(
                escape_sql_string_literal(s).as_ref(),
                s.replace('\'', "''"),
                "mismatch for input {s:?}"
            );
        }
    }
}
