//! Naming helpers for relation enums, fields, and FK attribute rendering.

use std::collections::HashSet;

use super::super::imports::{sanitize_field_name, sanitize_type_name, to_pascal_case};

/// Generate a relation enum name from foreign key column names.
/// For "`creator_user_id`", generates "`CreatorUser`".
/// For composite FKs like [`org_id`, `user_id`], generates `OrgUser`.
pub(in crate::seaorm) fn generate_relation_enum_name<T: AsRef<str>>(columns: &[T]) -> String {
    // Take the first column and remove common FK suffixes like "_id"
    let first_col = columns[0].as_ref();
    let without_id = first_col.strip_suffix("_id").unwrap_or(first_col);

    sanitize_type_name(&to_pascal_case(without_id))
}

pub(in crate::seaorm) fn unique_relation_enum_name(
    preferred: String,
    source_table: &str,
    base_relation_enum: &str,
    used_relation_enums: &HashSet<String>,
) -> String {
    if !used_relation_enums.contains(&preferred) {
        return preferred;
    }

    // The sanitized pascal form of `source_table` is a loop-invariant: compute
    // it once and reuse it for the source-prefixed candidate and every indexed
    // candidate, instead of re-allocating the identical prefix per attempt.
    let prefix = sanitize_type_name(&to_pascal_case(source_table));

    let source_prefixed = format!("{prefix}{base_relation_enum}");
    if !used_relation_enums.contains(&source_prefixed) {
        return source_prefixed;
    }

    let mut index = 2;
    loop {
        let candidate = format!("{prefix}{base_relation_enum}{index}");
        if !used_relation_enums.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

/// Infer a field name from a single FK column.
/// For "`creator_user_id`" with to="id", tries "`creator_user`" first.
/// If the FK column still follows common suffix naming like `_id`/`_idx`,
/// remove those as fallbacks for intuitive relation names.
/// If that ends with the table name, use the full column name (without the to suffix).
/// Otherwise, fall back to the table name.
///
/// Examples:
/// - FK column: "`creator_user_id`", table: "user", to: "id" -> "`creator_user`"
/// - FK column: "`creator_user_idx`", table: "user", to: "idx" -> "`creator_user`"
/// - FK column: "`user_id`", table: "user", to: "id" -> "user" (falls back to table name)
/// - FK column: "`order_id`", table: "order", to: "`order_number`" -> "order"
/// - FK column: "`order_idx`", table: "order", to: "`order_number`" -> "order"
/// - FK column: "`org_id`", table: "user", to: "id" -> "org"
pub(in crate::seaorm) fn infer_field_name_from_fk_column(
    fk_column: &str,
    table_name: &str,
    to: &str,
) -> String {
    let table_lower = table_name.to_lowercase();
    let to_lower = to.to_lowercase();

    // Remove the "to" suffix from FK column (e.g., "user_id" for to="id", "user_idx" for to="idx").
    // If FK column still uses common suffixes like "*_id"/"*_idx", strip them as fallbacks.
    let to_suffix = format!("_{to}");
    let without_suffix = fk_column
        .strip_suffix(&to_suffix)
        .or_else(|| fk_column.strip_suffix("_id"))
        .or_else(|| fk_column.strip_suffix("_idx"))
        .unwrap_or(fk_column);

    let sanitized = sanitize_field_name(without_suffix);
    let sanitized_lower = sanitized.to_lowercase();

    // If the FK column exactly matches the referenced column name, treat it as a natural-key
    // relation and expose the target entity name instead of the raw column name.
    // Also handle compact forms like `username` for `user.name`.
    // The second disjunct checks whether `sanitized_lower` equals `table_lower`
    // concatenated with `to_lower` (e.g. "username" for table "user", col "name").
    // Compare allocation-free via a prefix strip instead of building a joined
    // `String` on every call: it is exactly equal iff `sanitized_lower` starts
    // with `table_lower` and the remainder is `to_lower`.
    if sanitized_lower == to_lower
        || sanitized_lower.strip_prefix(table_lower.as_str()) == Some(to_lower.as_str())
    {
        return sanitize_field_name(table_name);
    }

    // If the sanitized name is exactly the table name (e.g., "user_id" -> "user" for table "user"),
    // we need to fall back to the table name for proper disambiguation.
    // Otherwise, use the inferred sanitized name from the column — this naturally
    // covers compound forms like "creator_user" for table "user".
    if sanitized_lower == table_lower {
        sanitize_field_name(table_name)
    } else {
        sanitized
    }
}

pub(in crate::seaorm) fn pluralize(name: &str) -> String {
    if name.ends_with('s') {
        name.to_string()
    } else if name.ends_with('y') && !ends_with_vowel_y(name) {
        // Build `<stem>ies` directly into one exact-size buffer instead of
        // routing through the `format!` formatter machinery (mirrors the
        // buffer-push idiom used by `fk_attr_value` / `quote_idents`). The
        // `strip_suffix('y')` is guarded by `ends_with('y')` above, so
        // `unwrap_or(name)` is dead-defensive but harmless. Byte-identical.
        let stem = name.strip_suffix('y').unwrap_or(name);
        let mut out = String::with_capacity(stem.len() + 3);
        out.push_str(stem);
        out.push_str("ies");
        out
    } else {
        format!("{name}s")
    }
}

/// True when the character *before* the trailing `y` is a vowel (e.g. `day`,
/// `key`, `boy`, `guy`). The sole caller (`pluralize`) already gates this on
/// `name.ends_with('y')`, so this helper owns only the "preceding char is a
/// vowel" test — a single lookup of the character before the trailing `y`. The
/// vowel set is exactly `{a, e, o, u}` (not `i`), so `iy` still pluralizes to
/// `ies`; a bare `"y"` (no preceding char) is treated as a consonant-`y` and
/// pluralizes to `ys`. Byte-identical to the prior chain.
fn ends_with_vowel_y(name: &str) -> bool {
    name.chars()
        .rev()
        .nth(1)
        .is_some_and(|c| matches!(c, 'a' | 'e' | 'o' | 'u'))
}

/// Render the `from = …` / `to = …` value of a `belongs_to` attribute.
///
/// `sea-orm` parses these as Rust paths and `PascalCase`s them into `Column`
/// variants, so they name model fields rather than database columns — an
/// escaped column has to appear here under its escaped name.
pub(super) fn fk_attr_value<T: AsRef<str>>(cols: &[T]) -> String {
    if let [only] = cols {
        return sanitize_field_name(only.as_ref());
    }

    // Write the sanitized fields straight into one pre-sized buffer instead of
    // collecting them into a throwaway `Vec<String>` just to `join(", ")` it
    // (mirrors the `quote_idents` idiom in vespertide-query). Output is
    // byte-identical: `(a, b, c)`.
    let content_len: usize = cols.iter().map(|c| c.as_ref().len()).sum();
    let mut out = String::with_capacity(content_len + 2 * cols.len());
    out.push('(');
    for (i, c) in cols.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&sanitize_field_name(c.as_ref()));
    }
    out.push(')');
    out
}
