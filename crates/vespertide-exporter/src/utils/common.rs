//! Cross-language helpers shared by every ORM exporter backend.

use vespertide_core::{NumValue, ReferenceAction, TableConstraint, TableDef};

/// Join items as a double-quoted, comma-separated list: `"a", "b", "c"`.
///
/// Consolidates the quoted-comma-join pattern previously copy-pasted across
/// the JPA, `SeaORM`, `SQLAlchemy` and `SQLModel` renderers, and builds the
/// result in a single buffer instead of collecting an intermediate
/// `Vec<String>` per call site.
pub(crate) fn join_quoted<T: AsRef<str>>(items: &[T]) -> String {
    // Pre-size exactly: each item contributes 2 quotes + its own length, and
    // every item after the first adds a 2-byte ", " separator. Mirrors the
    // `String::with_capacity` + buffer-push convention used by the sibling
    // helpers in `query/helpers.rs`, `vespertide-naming`, and
    // `seaorm/relations/naming.rs`. Output stays byte-identical.
    let content_len: usize = items.iter().map(|i| i.as_ref().len()).sum();
    let capacity = content_len + 2 * items.len() + 2 * items.len().saturating_sub(1);
    let mut out = String::with_capacity(capacity);
    for item in items {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push('"');
        out.push_str(item.as_ref());
        out.push('"');
    }
    out
}

/// Append one comma-separated attribute `fragment` into `buf`, inserting a
/// `", "` separator before every fragment except the first.
///
/// Replaces the `Vec<String>` + `.join(", ")` pattern used to assemble ORM
/// column-attribute lists (`@Column(name = ..., nullable = false)`,
/// `mapped_column(String, primary_key=True)`): fragments are written straight
/// into a single buffer with no intermediate `Vec` or per-fragment `String`
/// allocations. Output is byte-identical to `fragments.join(", ")`.
pub(crate) fn push_attr(buf: &mut String, fragment: &str) {
    if !buf.is_empty() {
        buf.push_str(", ");
    }
    buf.push_str(fragment);
}

/// Join FK-target columns as a double-quoted, `ref_table`-qualified,
/// comma-separated list: `"tbl.col1", "tbl.col2"`.
///
/// Consolidates the identical `ForeignKeyConstraint([...], [...])` target
/// rendering previously copy-pasted across the `SQLAlchemy` and `SQLModel`
/// renderers, and builds the result in a single buffer instead of collecting an
/// intermediate `Vec<String>` (one `String` per column) per call site.
pub(crate) fn join_qualified_refs(ref_table: &str, ref_cols: &[&str]) -> String {
    // Pre-size exactly: each column renders as `"<ref_table>.<col>"` — 2 quotes
    // + `ref_table.len()` + 1 dot + `col.len()` — and every column after the
    // first adds a 2-byte ", " separator. Matches the buffer-push pre-sizing
    // convention used across the workspace. Output stays byte-identical.
    let cols_len: usize = ref_cols.iter().map(|c| c.len()).sum();
    let per_col_fixed = 2 + ref_table.len() + 1;
    let capacity = per_col_fixed * ref_cols.len() + cols_len + 2 * ref_cols.len().saturating_sub(1);
    let mut out = String::with_capacity(capacity);
    for col in ref_cols {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push('"');
        out.push_str(ref_table);
        out.push('.');
        out.push_str(col);
        out.push('"');
    }
    out
}

/// Quote `value` as a double-quoted string literal.
///
/// Backslashes, quotes and the line terminators are escaped so a database name
/// or enum value containing any of them cannot end the literal early. The
/// escapes are the ones TypeScript and Go share, so every literal the Drizzle
/// and GORM renderers emit — table names, column names, enum values — goes
/// through here.
pub(crate) fn string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Strip one matching pair of surrounding quotes from a SQL literal.
///
/// Only an outer pair is removed, so quotes *inside* the literal survive:
/// trimming per character would turn `'say "hi"'` into `say "hi`, silently
/// dropping the closing quote. Input without a matching pair is returned
/// unchanged.
pub(crate) fn unquote(s: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(inner) = s
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    s
}

/// The stored value of the integer-enum variant named `name`, if there is one.
/// A model may write an integer enum's default as the variant name; the column
/// stores the value.
pub(crate) fn integer_enum_variant_value(variants: &[NumValue], name: &str) -> Option<i64> {
    variants.iter().find(|v| v.name == name).map(|v| v.value)
}

/// `JSONB` is the one custom column type the backends map to a native JSON
/// type instead of a plain string; the model may spell it in any case.
pub(crate) fn is_jsonb_custom_type(custom_type: &str) -> bool {
    custom_type.eq_ignore_ascii_case("JSONB")
}

/// A composite (multi-column) foreign key: its owning columns, target and
/// referential actions. Backends with no native composite relation
/// (SQLAlchemy, SQLModel) surface it as a comment; GORM renders it as
/// a relation field with comma-separated `foreignKey`/`references`.
pub(crate) struct CompositeFk<'a> {
    pub(crate) local_cols: Vec<&'a str>,
    pub(crate) ref_table: &'a str,
    pub(crate) ref_cols: Vec<&'a str>,
    pub(crate) on_delete: Option<&'a ReferenceAction>,
    pub(crate) on_update: Option<&'a ReferenceAction>,
}

pub(crate) fn collect_composite_fks(table: &TableDef) -> Vec<CompositeFk<'_>> {
    table
        .constraints
        .iter()
        .filter_map(|constraint| match constraint {
            TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                on_delete,
                on_update,
                ..
            } if columns.len() > 1 && columns.len() == ref_columns.len() => Some(CompositeFk {
                local_cols: columns.iter().map(AsRef::as_ref).collect(),
                ref_table: ref_table.as_str(),
                ref_cols: ref_columns.iter().map(AsRef::as_ref).collect(),
                on_delete: on_delete.as_ref(),
                on_update: on_update.as_ref(),
            }),
            _ => None,
        })
        .collect()
}

/// Claim a relation field name, recording it in `taken` so later fields
/// avoid it. Seed `taken` with the table's column field names first — relation
/// names are derived from column/table names, so a relation must not take a
/// column's name (nor an earlier relation's).
pub(crate) fn claim_field_name(
    preferred: String,
    taken: &mut std::collections::HashSet<String>,
) -> String {
    let chosen = first_unused(preferred, taken);
    taken.insert(chosen.clone());
    chosen
}

/// Claim a file-scope binding name, recording it in `taken`. Unlike
/// [`claim_field_name`], a collision takes a bare numeric suffix (`name2`,
/// `name3`, …) — the `_rel` step is a relation-field convention that would
/// mislead on a table or type binding.
pub(crate) fn claim_binding(
    preferred: String,
    taken: &mut std::collections::HashSet<String>,
) -> String {
    if taken.insert(preferred.clone()) {
        return preferred;
    }
    let mut n = 2usize;
    loop {
        let candidate = format!("{preferred}{n}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// `preferred` if free, then `{preferred}_rel`, then numbered variants. `_rel`
/// comes before the numbers so the names already emitted for FK fields that
/// clash with their own column stay unchanged.
fn first_unused(preferred: String, taken: &std::collections::HashSet<String>) -> String {
    if !taken.contains(&preferred) {
        return preferred;
    }

    let suffixed = format!("{preferred}_rel");
    if !taken.contains(&suffixed) {
        return suffixed;
    }

    let mut index = 2;
    loop {
        let candidate = format!("{preferred}_rel{index}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn empty_slice_yields_empty_string() {
        assert_eq!(join_quoted::<&str>(&[]), "");
    }

    #[test]
    fn single_item_is_quoted_without_separator() {
        assert_eq!(join_quoted(&["id"]), "\"id\"");
    }

    #[test]
    fn multiple_items_are_comma_separated() {
        assert_eq!(join_quoted(&["a", "b", "c"]), "\"a\", \"b\", \"c\"");
    }

    #[test]
    fn qualified_refs_empty_slice_yields_empty_string() {
        assert_eq!(join_qualified_refs("user", &[]), "");
    }

    #[test]
    fn qualified_refs_single_column_is_table_qualified() {
        assert_eq!(join_qualified_refs("user", &["id"]), "\"user.id\"");
    }

    #[test]
    fn qualified_refs_multiple_columns_are_comma_separated() {
        assert_eq!(
            join_qualified_refs("account", &["tenant_id", "id"]),
            "\"account.tenant_id\", \"account.id\""
        );
    }

    #[test]
    fn push_attr_matches_join_semantics() {
        let mut buf = String::new();
        push_attr(&mut buf, "name = \"id\"");
        assert_eq!(buf, "name = \"id\"");
        push_attr(&mut buf, "nullable = false");
        push_attr(&mut buf, "unique = true");
        assert_eq!(buf, "name = \"id\", nullable = false, unique = true");
        assert_eq!(
            buf,
            ["name = \"id\"", "nullable = false", "unique = true"].join(", ")
        );
    }

    #[test]
    fn push_attr_first_fragment_has_no_leading_separator() {
        let mut buf = String::new();
        push_attr(&mut buf, "String");
        assert_eq!(buf, "String");
    }

    /// Bindings suffix numerically — no `_rel` step — and each claim records
    /// itself, so a third claimant walks past the second's suffix.
    #[test]
    fn claim_binding_suffixes_numerically() {
        let mut taken = std::collections::HashSet::new();
        assert_eq!(claim_binding("user".to_string(), &mut taken), "user");
        assert_eq!(claim_binding("user".to_string(), &mut taken), "user2");
        assert_eq!(claim_binding("user".to_string(), &mut taken), "user3");
    }

    #[rstest]
    #[case::single_quoted("'draft'", "draft")]
    #[case::double_quoted("\"draft\"", "draft")]
    #[case::inner_quotes_survive("'say \"hi\"'", "say \"hi\"")]
    #[case::doubled_sql_escape("'it''s'", "it''s")]
    #[case::unquoted("draft", "draft")]
    #[case::mismatched_pair("\"draft'", "\"draft'")]
    #[case::opening_only("'draft", "'draft")]
    #[case::lone_quote("'", "'")]
    #[case::empty("", "")]
    fn unquote_removes_only_a_matching_outer_pair(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(unquote(input), expected);
    }

    #[rstest]
    #[case::plain("users", r#""users""#)]
    #[case::double_quote("say \"hi\"", r#""say \"hi\"""#)]
    #[case::backslash("back\\slash", r#""back\\slash""#)]
    #[case::newline("two\nlines", r#""two\nlines""#)]
    #[case::carriage_return("a\rb", r#""a\rb""#)]
    #[case::tab("a\tb", r#""a\tb""#)]
    #[case::empty("", r#""""#)]
    fn string_literal_escapes_literal_terminators(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(string_literal(input), expected);
    }
}
