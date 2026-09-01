//! Backend adaptation for `fill_with` / backfill values.
//!
//! `fill_with` is a **raw SQL expression slot**: whatever the user wrote is
//! spliced into the emitted `UPDATE` / `INSERT ... SELECT` verbatim (via
//! `Expr::cust`). That is a different contract from a column DEFAULT, which
//! [`convert_default_for_backend`] was written for — a single literal or
//! function call it is free to canonicalise.
//!
//! Running an expression through the DEFAULT path corrupted it. The
//! PostgreSQL-cast branch split at the *first* `::`, lower-cased everything
//! after it, and re-joined the halves, so
//!
//! ```sql
//! (CASE WHEN plan_key::text = 'API' THEN 'MONTHLY_QUOTA' ELSE 'SEAT' END)::billing_metric
//! ```
//!
//! was emitted with `'api'` / `'monthly_quota'` / `'seat'` — the comparison
//! never matched (silent no-op backfill) and the lower-cased token was not a
//! valid enum label, so the cast failed. On MySQL and SQLite the statement was
//! truncated at the split point outright.
//!
//! The rule enforced here: **never mutate user SQL.**

use super::helpers::{
    TIMESTAMP_FUNCTION_SPELLINGS, UUID_FUNCTION_SPELLINGS, convert_default_for_backend,
    find_last_top_level_cast, matches_any_spelling,
};
use super::types::DatabaseBackend;

/// Keywords that only occur in a *composite* SQL expression.
///
/// Deliberately excludes the short, label-like keywords (`and`, `or`, `not`,
/// `in`, `like`): an enum label such as `'in progress'` must stay on the
/// convertible path, or MySQL would receive its `::` cast verbatim.
const COMPOSITE_SQL_KEYWORDS: [&str; 11] = [
    "case", "when", "then", "else", "end", "select", "from", "where", "union", "join", "between",
];

/// Adapt a `fill_with` / backfill expression for `backend`.
///
/// * PostgreSQL — the dialect `fill_with` is authored in — always receives the
///   value **verbatim**.
/// * Other backends are only allowed to rewrite a value that is unambiguously
///   a single simple literal (or one of the portable function spellings).
///   Anything composite passes through untouched.
#[must_use]
pub(crate) fn convert_fill_with_for_backend(fill: &str, backend: DatabaseBackend) -> String {
    if backend == DatabaseBackend::Postgres || !is_simple_literal_fill(fill) {
        return fill.to_string();
    }
    convert_default_for_backend(fill, backend)
}

/// Whether `fill` is safe to hand to [`convert_default_for_backend`], i.e. it
/// is either a whole-string portable function spelling (`NOW()`,
/// `gen_random_uuid()`, …) or a single simple literal / identifier optionally
/// carrying one trailing `::type` cast.
fn is_simple_literal_fill(fill: &str) -> bool {
    let trimmed = fill.trim();
    if matches_any_spelling(trimmed, &UUID_FUNCTION_SPELLINGS)
        || matches_any_spelling(trimmed, &TIMESTAMP_FUNCTION_SPELLINGS)
    {
        return true;
    }
    if contains_composite_keyword(trimmed) {
        return false;
    }
    let value = match find_last_top_level_cast(trimmed) {
        Some(split) => trimmed[..split].trim(),
        None => trimmed,
    };
    is_single_sql_atom(value)
}

/// Whether `value` is exactly one complete quoted string literal, or one bare
/// token free of whitespace, parentheses, commas and quotes.
fn is_single_sql_atom(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if value.starts_with('\'') {
        return is_one_complete_quoted_literal(value);
    }
    !value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '(' | ')' | ',' | ';' | '\'' | '"'))
}

/// Whether `value` is a single quoted literal with nothing trailing it.
///
/// Any byte outside the literal disqualifies it, so `'a' || 'b'` is rejected,
/// while the `''` escape in `'it''s'` keeps the literal open — the pair closes
/// and immediately reopens it.
fn is_one_complete_quoted_literal(value: &str) -> bool {
    let mut in_quote = false;
    for byte in value.bytes() {
        if byte == b'\'' {
            in_quote = !in_quote;
        } else if !in_quote {
            return false;
        }
    }
    !in_quote
}

/// Whether `value` contains a [`COMPOSITE_SQL_KEYWORDS`] entry as a whole word.
///
/// Splitting on non-identifier bytes keeps `weekend_total` distinct from `end`,
/// which is what lets a lone identifier stay on the convertible path.
fn contains_composite_keyword(value: &str) -> bool {
    value
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|word| matches_any_spelling(word, &COMPOSITE_SQL_KEYWORDS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// The three reported corruptions, at the unit level: every backend must
    /// hand back the expression byte-for-byte.
    #[rstest]
    #[case::enum_cast(
        "(CASE WHEN plan_key::text = 'API' THEN 'MONTHLY_QUOTA' ELSE 'SEAT' END)::billing_metric"
    )]
    #[case::json_array(
        "CASE WHEN device_os = 'win' THEN json_build_array('WINDOWS', device_family::text) ELSE '[]'::json END"
    )]
    #[case::cast_inside_quotes("CASE WHEN plan_tag = 'legacy::v1' THEN 1 ELSE 2 END::integer")]
    fn composite_expressions_survive_verbatim(#[case] fill: &str) {
        for backend in [
            DatabaseBackend::Postgres,
            DatabaseBackend::MySql,
            DatabaseBackend::Sqlite,
        ] {
            assert_eq!(
                convert_fill_with_for_backend(fill, backend),
                fill,
                "{backend:?} must not rewrite a fill_with expression"
            );
        }
    }

    /// PostgreSQL is the authoring dialect, so even a value the DEFAULT path
    /// would canonicalise is emitted exactly as written.
    #[rstest]
    #[case("NOW()")]
    #[case("gen_random_uuid()")]
    #[case("'[]'::json")]
    #[case("0")]
    fn postgres_never_rewrites(#[case] fill: &str) {
        assert_eq!(
            convert_fill_with_for_backend(fill, DatabaseBackend::Postgres),
            fill
        );
    }

    #[rstest]
    #[case::now_mysql("NOW()", DatabaseBackend::MySql, "CURRENT_TIMESTAMP")]
    #[case::now_sqlite("NOW()", DatabaseBackend::Sqlite, "CURRENT_TIMESTAMP")]
    #[case::uuid_mysql("gen_random_uuid()", DatabaseBackend::MySql, "(UUID())")]
    #[case::uuid_sqlite(
        "gen_random_uuid()",
        DatabaseBackend::Sqlite,
        "lower(hex(randomblob(16)))"
    )]
    #[case::json_cast_mysql("'[]'::json", DatabaseBackend::MySql, "CAST('[]' AS JSON)")]
    #[case::json_cast_sqlite("'[]'::json", DatabaseBackend::Sqlite, "'[]'")]
    #[case::int_cast_mysql("0::integer", DatabaseBackend::MySql, "CAST(0 AS SIGNED)")]
    #[case::identifier_cast_sqlite("legacy_id::text", DatabaseBackend::Sqlite, "legacy_id")]
    #[case::empty_literal_mysql("''", DatabaseBackend::MySql, "''")]
    #[case::plain_number_sqlite("0", DatabaseBackend::Sqlite, "0")]
    fn simple_literals_still_convert_cross_backend(
        #[case] fill: &str,
        #[case] backend: DatabaseBackend,
        #[case] expected: &str,
    ) {
        assert_eq!(convert_fill_with_for_backend(fill, backend), expected);
    }

    #[rstest]
    #[case::plain_number("0", true)]
    #[case::quoted_literal("'active'", true)]
    #[case::quoted_literal_with_space("'in progress'", true)]
    #[case::quoted_literal_with_keyword_inside("'not_started'::user_status", true)]
    #[case::identifier_cast("legacy_id::text", true)]
    #[case::portable_function("NOW()", true)]
    #[case::nested_uuid_function("lower(hex(randomblob(16)))", true)]
    #[case::empty("", false)]
    #[case::whitespace_only("   ", false)]
    #[case::function_call("json_build_array('a')", false)]
    #[case::concatenation("'a' || 'b'", false)]
    #[case::bare_keyword("END", false)]
    #[case::case_expression("CASE WHEN a = 1 THEN 'x' ELSE 'y' END", false)]
    #[case::parenthesised_cast("(a + b)::integer", false)]
    #[case::unterminated_literal("'oops", false)]
    fn simple_literal_classification(#[case] fill: &str, #[case] expected: bool) {
        assert_eq!(is_simple_literal_fill(fill), expected, "input: {fill}");
    }

    /// Whole-word matching is what keeps a lone identifier convertible: an
    /// enum label like `not_started` or a column like `weekend_total` must not
    /// be read as a keyword and pushed onto the verbatim path, where MySQL
    /// would choke on the trailing `::` cast.
    #[test]
    fn composite_keywords_match_whole_words_only() {
        assert!(contains_composite_keyword(
            "CASE WHEN a = 1 THEN 'x' ELSE 'y' END"
        ));
        assert!(contains_composite_keyword("SELECT 1 FROM t"));
        assert!(!contains_composite_keyword("'not_started'::user_status"));
        assert!(!contains_composite_keyword("weekend_total::integer"));
        assert!(!contains_composite_keyword("''"));
    }
}
