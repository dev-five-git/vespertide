//! Code actions — quick refactors for the column under the cursor.
//!
//! V1 surface area:
//! * Toggle `primary_key`, `unique`, `index` on a column (add when absent,
//!   remove when present).
//! * Toggle nullability (`nullable: true` ↔ `nullable: false`).
//!
//! All edits are produced as JSON-aware byte-level [`DomainTextEdit`]s. JSON
//! is the format we drive most often and where the structural rules are
//! tightest — YAML support can grow from the same data model later.
//!
//! Returned [`DomainCodeAction`]s carry the title (what the editor lists)
//! and a small set of edits scoped to the file the cursor is in. Workspace
//! edits are out of scope here; rename / references handle those.

use std::ops::Range;

use crate::parser::DocumentFormat;
use crate::rename::DomainTextEdit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainCodeAction {
    pub title: String,
    pub kind: CodeActionKind,
    pub edits: Vec<DomainTextEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeActionKind {
    /// `refactor.rewrite` family — for boolean flag toggles.
    Refactor,
}

/// Compute available code actions for the byte range under the cursor.
#[must_use]
pub fn compute(
    source: &str,
    format: DocumentFormat,
    tree: Option<&tree_sitter::Tree>,
    byte_range: Range<usize>,
) -> Vec<DomainCodeAction> {
    // YAML structural edits are non-trivial (indent-aware). V1 only emits
    // actions for JSON — YAML can grow from the same data model later.
    if format != DocumentFormat::Json {
        return Vec::new();
    }
    let mut actions = Vec::new();
    if let Some(tree) = tree {
        let source_bytes = source.as_bytes();
        // Column-scoped actions fire only when the cursor sits inside a column
        // object (the `columns` array). CHECK constraints live in the
        // table-level `constraints` array, so their actions are gated
        // separately below.
        if let Some(column) =
            enclosing_column_object(tree.root_node(), source_bytes, byte_range.start)
        {
            actions.extend(flag_toggles(column, source_bytes));
            actions.extend(type_conversions(column, source_bytes));
            actions.extend(enum_extraction(column, source_bytes));
            actions.extend(fk_skeleton(column, source_bytes));
        }
        actions.extend(check_expr_actions(tree, source_bytes, byte_range.start));
    }
    actions
}

/// Quick-fixes available when the cursor sits inside a table-level CHECK
/// `expr` string. Currently a single deterministic refactor: swapping the
/// bounds of a reversed `BETWEEN low AND high` (`low > high`), which pairs
/// with the `check-between-reversed` hard-error diagnostic.
fn check_expr_actions(
    tree: &tree_sitter::Tree,
    source: &[u8],
    byte_offset: usize,
) -> Vec<DomainCodeAction> {
    if let Some(ctx) = crate::check_expr_locate::find_check_expr_at(tree, source, byte_offset)
        && let Some(expr_text) = source
            .get(ctx.inner.clone())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    {
        swap_reversed_between(expr_text, ctx.inner.start)
    } else {
        Vec::new()
    }
}

/// Scan the CHECK expression's token stream for `BETWEEN low AND high`
/// nodes whose literal bounds are reversed, and emit a swap edit for each.
/// `base` is the absolute byte offset of the first character of `expr_text`
/// within the source document (i.e. just inside the opening quote).
///
/// Mirrors the planner's F-novel-15 detection semantics: integer/float
/// (and cross-numeric) compared numerically, string compared
/// lexicographically, anything mixed or non-orderable (bool/null) skipped.
/// `NOT BETWEEN` is skipped — a reversed `NOT BETWEEN` is always-true and
/// therefore harmless.
fn swap_reversed_between(expr_text: &str, base: usize) -> Vec<DomainCodeAction> {
    use vespertide_planner::{CheckTokenKind, lex_check_expr};

    let tokens = lex_check_expr(expr_text);
    let mut out = Vec::new();
    for i in 0..tokens.len() {
        // tokens[i] must be the BETWEEN keyword.
        if tokens[i].kind != CheckTokenKind::Keyword
            || !token_text(expr_text, &tokens[i]).is_some_and(|t| t.eq_ignore_ascii_case("between"))
        {
            continue;
        }
        // Skip `NOT BETWEEN` — reversed form is always-true (harmless).
        if i >= 1
            && tokens[i - 1].kind == CheckTokenKind::Keyword
            && token_text(expr_text, &tokens[i - 1]).is_some_and(|t| t.eq_ignore_ascii_case("not"))
        {
            continue;
        }
        // Expect: low literal, AND keyword, high literal.
        if let (Some(low), Some(andt), Some(high)) =
            (tokens.get(i + 1), tokens.get(i + 2), tokens.get(i + 3))
            && is_literal(low.kind)
            && andt.kind == CheckTokenKind::Keyword
            && token_text(expr_text, andt).is_some_and(|t| t.eq_ignore_ascii_case("and"))
            && is_literal(high.kind)
            // Boundary-safe: `token_text` uses `str::get`, so a span that is not
            // on a UTF-8 char boundary (defensive — the lexer emits ASCII-aligned
            // spans today) yields `None` and we simply skip rather than panic.
            && let (Some(low_text), Some(high_text)) =
                (token_text(expr_text, low), token_text(expr_text, high))
            && literal_greater(low_text, high_text)
        {
            out.push(DomainCodeAction {
                title: "Swap reversed BETWEEN bounds".to_string(),
                kind: CodeActionKind::Refactor,
                edits: vec![
                    DomainTextEdit {
                        byte_range: (base + low.span.start)..(base + low.span.end),
                        new_text: high_text.to_string(),
                    },
                    DomainTextEdit {
                        byte_range: (base + high.span.start)..(base + high.span.end),
                        new_text: low_text.to_string(),
                    },
                ],
            });
        }
    }
    out
}

/// Boundary-safe slice of a lexer token's text. Returns `None` when the span
/// is out of range or does not fall on a UTF-8 char boundary, so hostile or
/// malformed CHECK expressions can never panic the LSP.
fn token_text<'a>(expr_text: &'a str, token: &vespertide_planner::CheckToken) -> Option<&'a str> {
    expr_text.get(token.span.clone())
}

fn is_literal(kind: vespertide_planner::CheckTokenKind) -> bool {
    matches!(
        kind,
        vespertide_planner::CheckTokenKind::Number | vespertide_planner::CheckTokenKind::String
    )
}

/// True iff `a` is *demonstrably* greater than `b` under a conservative
/// ordering (mirrors fault F-novel-15). Only three unambiguous cases order:
/// two `i64`-parseable integers (exact); two float literals (each containing
/// `.`/`e`/`E`) compared as `f64`; or two single-quoted SQL string literals
/// (lexicographic on the as-written text — the equal leading quote makes this
/// equivalent to comparing contents). Everything else — mixed numeric/string,
/// bool, null, or an integer too large for `i64` (where `f64` rounding could
/// mis-order) — folds to `false`, so an ambiguous pair never produces a
/// (possibly wrong) swap suggestion.
fn literal_greater(a: &str, b: &str) -> bool {
    if let (Ok(x), Ok(y)) = (a.parse::<i64>(), b.parse::<i64>()) {
        return x > y;
    }
    // Float path ONLY for genuine float literals (contain a fraction/exponent).
    // This avoids `f64`-rounding mis-ordering of huge integers that overflow
    // `i64` — those fall through to `false` (no swap offered).
    if is_float_literal(a)
        && is_float_literal(b)
        && let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>())
    {
        return x > y;
    }
    // Two single-quoted SQL string literals: lexicographic on as-written text.
    if a.starts_with('\'') && b.starts_with('\'') {
        return a > b;
    }
    false
}

/// A numeric literal with a fractional or exponent part (so `f64` ordering is
/// the intended comparison, not an `i64` that merely overflowed).
fn is_float_literal(s: &str) -> bool {
    s.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') && s.parse::<f64>().is_ok()
}

fn flag_toggles(column: tree_sitter::Node<'_>, source: &[u8]) -> Vec<DomainCodeAction> {
    let mut actions = Vec::new();
    for (flag, on_title, off_title) in [
        (
            "primary_key",
            "Mark column as primary key",
            "Unmark primary key",
        ),
        ("unique", "Mark column as unique", "Remove unique"),
        ("index", "Add index to column", "Remove index"),
    ] {
        actions.extend(toggle_bool_flag(column, source, flag, on_title, off_title));
    }
    actions.extend(toggle_nullable(column, source));
    actions
}

/// Convert a simple string column type (`"text"`, `"integer"`) into its
/// parametric complex form (`{kind: varchar, length: 255}`,
/// `{kind: numeric, precision: 10, scale: 2}`). Only fires when the
/// existing type is a plain string — complex object types are already
/// parametrised, so offering the conversion again would just clobber
/// the user's `length` / `precision`.
fn type_conversions(column: tree_sitter::Node<'_>, source: &[u8]) -> Vec<DomainCodeAction> {
    let mut out = Vec::new();
    if let Some(type_value) = find_pair(column, source, "type").and_then(|pair| pair.named_child(1))
        // Only operate on a simple string type; object types are already
        // parametric.
        && type_value.kind() == "string"
        && let Some(text) = source
            .get(type_value.byte_range())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    {
        match strip_quotes(text) {
            // Variable-width strings: offer varchar(255).
            "text" | "varchar" | "char" => {
                out.push(replace_type_action(
                    "Convert to varchar(255)",
                    type_value,
                    r#"{"kind":"varchar","length":255}"#,
                ));
            }
            // Whole-number numeric types: offer numeric(10,2) — typical money-ish default.
            "integer" | "big_int" | "small_int" | "real" | "double_precision" => {
                out.push(replace_type_action(
                    "Convert to numeric(10,2)",
                    type_value,
                    r#"{"kind":"numeric","precision":10,"scale":2}"#,
                ));
            }
            _ => {}
        }
    }
    out
}

fn replace_type_action(
    title: &str,
    type_value: tree_sitter::Node<'_>,
    new_value: &str,
) -> DomainCodeAction {
    DomainCodeAction {
        title: title.to_string(),
        kind: CodeActionKind::Refactor,
        edits: vec![DomainTextEdit {
            byte_range: type_value.byte_range(),
            new_text: new_value.to_string(),
        }],
    }
}

/// Promote a column's `default: "'literal'"` into a full enum type
/// definition. V1 is single-column: we synthesise an enum whose only
/// value is the existing default. The user can extend `values` after.
///
/// Skipped when:
///   * cursor's column has no `default` pair,
///   * default isn't a SQL string literal of the form `'…'`,
///   * column type isn't a simple string (we'd be clobbering object type).
fn enum_extraction(column: tree_sitter::Node<'_>, source: &[u8]) -> Vec<DomainCodeAction> {
    let default_inner = find_pair(column, source, "default")
        .and_then(|pair| pair.named_child(1))
        .filter(|default_value| default_value.kind() == "string")
        .and_then(|default_value| source.get(default_value.byte_range()))
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .map(strip_quotes)
        // Look for `'literal'` pattern — SQL single-quote literal inside JSON string.
        .and_then(|default_text| {
            default_text
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .filter(|s| !s.is_empty())
        });
    let type_value = find_pair(column, source, "type")
        .and_then(|pair| pair.named_child(1))
        .filter(|type_value| type_value.kind() == "string");

    if let Some(inner) = default_inner
        && let Some(type_value) = type_value
    {
        let column_name = find_pair(column, source, "name")
            .and_then(|p| p.named_child(1))
            .and_then(|v| source.get(v.byte_range()))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map_or("status", strip_quotes)
            .to_string();

        let enum_name = format!("{column_name}_kind");
        let new_type = format!(r#"{{"kind":"enum","name":"{enum_name}","values":["{inner}"]}}"#);
        vec![DomainCodeAction {
            title: format!("Extract default into enum `{enum_name}`"),
            kind: CodeActionKind::Refactor,
            edits: vec![DomainTextEdit {
                byte_range: type_value.byte_range(),
                new_text: new_type,
            }],
        }]
    } else {
        Vec::new()
    }
}

/// Insert a `foreign_key` skeleton when the column doesn't yet have one.
/// Reuses [`insert_pair_edit`] so we share the comma-aware insertion
/// behaviour with the boolean-flag toggles.
fn fk_skeleton(column: tree_sitter::Node<'_>, source: &[u8]) -> Vec<DomainCodeAction> {
    if find_pair(column, source, "foreign_key").is_some() {
        return Vec::new();
    }
    if let Some(edit) = insert_pair_edit(
        column,
        source,
        "foreign_key",
        r#"{"ref_table":"","ref_columns":["id"],"on_delete":"cascade"}"#,
    ) {
        vec![DomainCodeAction {
            title: "Add foreign_key skeleton".to_string(),
            kind: CodeActionKind::Refactor,
            edits: vec![edit],
        }]
    } else {
        Vec::new()
    }
}

fn toggle_bool_flag(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    flag: &str,
    title_when_adding: &str,
    title_when_removing: &str,
) -> Option<DomainCodeAction> {
    if let Some(pair) = find_pair(column, source, flag) {
        let value = pair.named_child(1)?;
        let value_text = std::str::from_utf8(&source[value.byte_range()]).ok()?;
        if value_text.trim() == "true" {
            return Some(DomainCodeAction {
                title: title_when_removing.to_string(),
                kind: CodeActionKind::Refactor,
                edits: vec![remove_pair_edit(column, pair, source)?],
            });
        }
        // Field exists but is `false` / something else — flip to true.
        return Some(DomainCodeAction {
            title: title_when_adding.to_string(),
            kind: CodeActionKind::Refactor,
            edits: vec![DomainTextEdit {
                byte_range: value.byte_range(),
                new_text: "true".to_string(),
            }],
        });
    }

    // Flag absent → insert it.
    Some(DomainCodeAction {
        title: title_when_adding.to_string(),
        kind: CodeActionKind::Refactor,
        edits: vec![insert_pair_edit(column, source, flag, "true")?],
    })
}

fn toggle_nullable(column: tree_sitter::Node<'_>, source: &[u8]) -> Option<DomainCodeAction> {
    let pair = find_pair(column, source, "nullable");
    if let Some(pair) = pair {
        let value = pair.named_child(1)?;
        let value_text = std::str::from_utf8(&source[value.byte_range()]).ok()?;
        let (next_value, title) = match value_text.trim() {
            "true" => ("false", "Make column NOT NULL"),
            "false" => ("true", "Allow NULL"),
            _ => return None,
        };
        return Some(DomainCodeAction {
            title: title.to_string(),
            kind: CodeActionKind::Refactor,
            edits: vec![DomainTextEdit {
                byte_range: value.byte_range(),
                new_text: next_value.to_string(),
            }],
        });
    }

    // Absent → assume the caller wants a non-nullable column (most common).
    Some(DomainCodeAction {
        title: "Make column NOT NULL".to_string(),
        kind: CodeActionKind::Refactor,
        edits: vec![insert_pair_edit(column, source, "nullable", "false")?],
    })
}

fn enclosing_column_object<'tree>(
    root: tree_sitter::Node<'tree>,
    source: &[u8],
    byte_offset: usize,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = root;
    while let Some(child) = child_containing_byte(current, byte_offset) {
        current = child;
    }
    // Walk back up to the smallest enclosing JSON object whose ancestor is
    // a `columns` array.
    let mut node = Some(current);
    while let Some(candidate) = node {
        if candidate.kind() == "object" && is_inside_columns_array(candidate, source) {
            return Some(candidate);
        }
        node = candidate.parent();
    }
    None
}

fn child_containing_byte(
    node: tree_sitter::Node<'_>,
    byte_offset: usize,
) -> Option<tree_sitter::Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.byte_range().contains(&byte_offset))
}

fn is_inside_columns_array(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "pair"
            && let Some(key) = candidate.named_child(0)
            && let Ok(text) = std::str::from_utf8(&source[key.byte_range()])
            && strip_quotes(text) == "columns"
        {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn find_pair<'tree>(
    object: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = object.walk();
    object.children(&mut cursor).find(|&child| {
        child.kind() == "pair"
            && child
                .named_child(0)
                .and_then(|key| std::str::from_utf8(&source[key.byte_range()]).ok())
                .map(strip_quotes)
                == Some(target_key)
    })
}

fn insert_pair_edit(
    column: tree_sitter::Node<'_>,
    source: &[u8],
    new_key: &str,
    new_value: &str,
) -> Option<DomainTextEdit> {
    let object_text = std::str::from_utf8(&source[column.byte_range()]).ok()?;
    let close_idx = object_text.rfind('}')?;
    let absolute_close = column.start_byte() + close_idx;

    // Inspect what comes immediately before the closing brace so we know
    // whether to emit a leading comma.
    let trimmed = object_text[..close_idx].trim_end();
    let needs_comma = trimmed.ends_with(|c: char| c != '{' && c != ',');
    let insertion = if needs_comma {
        format!(",\"{new_key}\":{new_value}")
    } else {
        format!("\"{new_key}\":{new_value}")
    };

    Some(DomainTextEdit {
        byte_range: absolute_close..absolute_close,
        new_text: insertion,
    })
}

fn remove_pair_edit(
    column: tree_sitter::Node<'_>,
    pair: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<DomainTextEdit> {
    // Decide which neighbouring comma to consume so the resulting JSON has
    // no `,,` or trailing `,}`.
    let object_text = std::str::from_utf8(&source[column.byte_range()]).ok()?;
    let object_start = column.start_byte();
    let pair_start = pair.start_byte() - object_start;
    let pair_end = pair.end_byte() - object_start;

    let before = &object_text[..pair_start];
    let after = &object_text[pair_end..];

    let trim_before = before.trim_end_matches(|c: char| c.is_whitespace());
    let trim_after = after.trim_start_matches(|c: char| c.is_whitespace());

    let removed_start;
    let removed_end;
    if trim_before.ends_with(',') {
        // Eat the leading comma + any whitespace between it and the pair.
        let comma_offset = trim_before.len() - 1;
        removed_start = object_start + comma_offset;
        removed_end = object_start + pair_end;
    } else if trim_after.starts_with(',') {
        // The pair is at the front; eat the comma that follows it.
        let comma_offset = pair_end + (after.len() - trim_after.len()) + 1;
        removed_start = object_start + pair_start;
        removed_end = object_start + comma_offset;
    } else {
        // Single pair object — just drop it.
        removed_start = object_start + pair_start;
        removed_end = object_start + pair_end;
    }

    Some(DomainTextEdit {
        byte_range: removed_start..removed_end,
        new_text: String::new(),
    })
}

fn strip_quotes(text: &str) -> &str {
    text.trim().trim_start_matches('"').trim_end_matches('"')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::test_support::parse_json as parse;

    #[test]
    fn add_primary_key_when_absent() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        let action = actions
            .iter()
            .find(|a| a.title == "Mark column as primary key")
            .expect("primary_key add action missing");
        assert_eq!(action.edits.len(), 1);
        let edit = &action.edits[0];
        assert_eq!(edit.new_text, r#","primary_key":true"#);
    }

    #[test]
    fn remove_primary_key_when_present() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","primary_key":true}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        let action = actions
            .iter()
            .find(|a| a.title == "Unmark primary key")
            .expect("primary_key remove action missing");
        let edit = &action.edits[0];
        // Confirm the edit produces a valid object when applied.
        let mut after = String::from(&src[..edit.byte_range.start]);
        after.push_str(&edit.new_text);
        after.push_str(&src[edit.byte_range.end..]);
        assert!(serde_json::from_str::<serde_json::Value>(&after).is_ok());
        assert!(!after.contains("primary_key"));
    }

    #[test]
    fn toggle_nullable_flips_value() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":false}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        let action = actions
            .iter()
            .find(|a| a.title == "Allow NULL")
            .expect("nullable toggle missing");
        let edit = &action.edits[0];
        assert_eq!(edit.new_text, "true");
        // Sanity check the byte_range covers `false` only.
        assert_eq!(&src[edit.byte_range.clone()], "false");
    }

    #[test]
    fn cursor_outside_column_returns_no_actions() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tree = parse(src);
        // Position cursor inside the table-level `name` value — NOT a column.
        let cursor = src.find(r#""name":"u""#).unwrap() + 9;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions.is_empty(),
            "no actions expected outside a column, got: {actions:?}"
        );
    }

    #[test]
    fn text_column_offers_varchar_conversion() {
        let src = r#"{"name":"u","columns":[{"name":"title","type":"text"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""title""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let convert = actions
            .iter()
            .find(|a| a.title == "Convert to varchar(255)")
            .expect("text → varchar action");
        let edit = &convert.edits[0];
        let mut after = String::from(&src[..edit.byte_range.start]);
        after.push_str(&edit.new_text);
        after.push_str(&src[edit.byte_range.end..]);
        assert!(after.contains(r#""kind":"varchar""#));
        serde_json::from_str::<serde_json::Value>(&after).expect("valid JSON");
    }

    #[test]
    fn integer_column_offers_numeric_conversion() {
        let src = r#"{"name":"u","columns":[{"name":"amount","type":"integer"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""amount""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let convert = actions
            .iter()
            .find(|a| a.title == "Convert to numeric(10,2)")
            .expect("integer → numeric action");
        let edit = &convert.edits[0];
        assert!(edit.new_text.contains(r#""kind":"numeric""#));
    }

    #[test]
    fn complex_type_already_does_not_offer_conversion() {
        let src = r#"{"name":"u","columns":[{"name":"t","type":{"kind":"varchar","length":100}}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""t""#).unwrap() + 1;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(actions.iter().all(|a| !a.title.starts_with("Convert to")));
    }

    #[test]
    fn extract_default_to_enum_offered_when_default_is_sql_literal() {
        let src =
            r#"{"name":"u","columns":[{"name":"status","type":"text","default":"'pending'"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""status""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let extract = actions
            .iter()
            .find(|a| a.title.starts_with("Extract default into enum"))
            .expect("enum extraction action");
        let edit = &extract.edits[0];
        assert!(edit.new_text.contains(r#""kind":"enum""#));
        assert!(edit.new_text.contains(r#""pending""#));
    }

    #[test]
    fn extract_default_to_enum_skipped_when_default_is_bare() {
        let src = r#"{"name":"u","columns":[{"name":"x","type":"integer","default":0}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""x""#).unwrap() + 1;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| !a.title.starts_with("Extract default"))
        );
    }

    #[test]
    fn add_foreign_key_skeleton_offered_when_absent() {
        let src = r#"{"name":"u","columns":[{"name":"author_id","type":"integer"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""author_id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let fk = actions
            .iter()
            .find(|a| a.title == "Add foreign_key skeleton")
            .expect("foreign_key skeleton action");
        let edit = &fk.edits[0];
        let mut after = String::from(&src[..edit.byte_range.start]);
        after.push_str(&edit.new_text);
        after.push_str(&src[edit.byte_range.end..]);
        assert!(after.contains(r#""foreign_key""#));
        serde_json::from_str::<serde_json::Value>(&after).expect("valid JSON");
    }

    #[test]
    fn add_foreign_key_skeleton_skipped_when_already_present() {
        let src = r#"{"name":"u","columns":[{"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""author_id""#).unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Add foreign_key skeleton")
        );
    }

    #[test]
    fn yaml_returns_no_actions_in_v1() {
        let src = "name: u\ncolumns:\n  - name: id\n    type: integer\n";
        let pool = ParserPool::new();
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let cursor = src.find("name: id").unwrap();
        let actions = compute(src, DocumentFormat::Yaml, Some(&tree), cursor..cursor);
        assert!(actions.is_empty(), "YAML actions are out of scope in V1");
    }

    // -- CHECK BETWEEN-swap quick-fix (F-novel-15 companion) --------------
    //
    // When the cursor sits inside a table-level CHECK `expr` whose
    // `BETWEEN low AND high` literal bounds are reversed (`low > high`),
    // offer a deterministic "Swap reversed BETWEEN bounds" refactor that
    // transposes the two literals. Pairs with the `check-between-reversed`
    // hard-error diagnostic.

    /// Apply `action`'s edits to `src` (front-to-back safe) and return the
    /// resulting document.
    fn apply(src: &str, action: &DomainCodeAction) -> String {
        let mut edits = action.edits.clone();
        edits.sort_by_key(|e| std::cmp::Reverse(e.byte_range.start));
        let mut out = src.to_string();
        for e in &edits {
            out.replace_range(e.byte_range.clone(), &e.new_text);
        }
        out
    }

    #[test]
    fn reversed_integer_between_offers_swap() {
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk","expr":"age BETWEEN 100 AND 0"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2; // inside the CHECK expr
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let swap = actions
            .iter()
            .find(|a| a.title == "Swap reversed BETWEEN bounds")
            .expect("BETWEEN swap action missing");
        let after = apply(src, swap);
        assert!(
            after.contains(r#""expr":"age BETWEEN 0 AND 100""#),
            "bounds must be swapped, got: {after}"
        );
        serde_json::from_str::<serde_json::Value>(&after).expect("valid JSON after swap");
    }

    #[test]
    fn correctly_ordered_between_offers_no_swap() {
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk","expr":"age BETWEEN 0 AND 100"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Swap reversed BETWEEN bounds"),
            "correctly-ordered BETWEEN must not offer a swap"
        );
    }

    #[test]
    fn not_between_reversed_offers_no_swap() {
        // `NOT BETWEEN 100 AND 0` is always-true (harmless) — no swap.
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk","expr":"age NOT BETWEEN 100 AND 0"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Swap reversed BETWEEN bounds"),
            "NOT BETWEEN reversed is harmless; no swap expected"
        );
    }

    #[test]
    fn reversed_string_between_offers_swap() {
        let src = r#"{"name":"u","columns":[{"name":"code","type":"text"}],"constraints":[{"type":"check","name":"chk","expr":"code BETWEEN 'z' AND 'a'"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let swap = actions
            .iter()
            .find(|a| a.title == "Swap reversed BETWEEN bounds")
            .expect("string BETWEEN swap action missing");
        let after = apply(src, swap);
        assert!(
            after.contains(r#""expr":"code BETWEEN 'a' AND 'z'""#),
            "string bounds must be swapped, got: {after}"
        );
        serde_json::from_str::<serde_json::Value>(&after).expect("valid JSON after swap");
    }

    #[test]
    fn mixed_type_between_offers_no_swap() {
        // int vs string boundary — not orderable, conservative skip.
        let src = r#"{"name":"u","columns":[{"name":"x","type":"integer"}],"constraints":[{"type":"check","name":"chk","expr":"x BETWEEN 100 AND 'a'"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Swap reversed BETWEEN bounds"),
            "mixed-type BETWEEN must not offer a swap"
        );
    }

    #[test]
    fn cursor_on_column_offers_no_between_swap() {
        // Cursor on the column declaration, not inside the CHECK expr.
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk","expr":"age BETWEEN 100 AND 0"}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""name":"age""#).unwrap() + 8;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Swap reversed BETWEEN bounds"),
            "BETWEEN swap must only fire inside the CHECK expr"
        );
    }

    #[test]
    fn reversed_string_between_with_non_ascii_does_not_panic_and_swaps() {
        // Multi-byte content inside the quoted string literals must not cause
        // a slicing panic, and the swap must still produce valid JSON.
        // 'ü...' (first byte 0xC3) > 'a...' (0x61) byte-wise → reversed → swap.
        let src = r#"{"name":"u","columns":[{"name":"code","type":"text"}],"constraints":[{"type":"check","name":"chk","expr":"code BETWEEN 'über' AND 'apple'"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        // Must not panic on multi-byte content inside the string literals.
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let swap = actions
            .iter()
            .find(|a| a.title == "Swap reversed BETWEEN bounds")
            .expect("non-ASCII string BETWEEN swap action missing");
        let after = apply(src, swap);
        serde_json::from_str::<serde_json::Value>(&after)
            .expect("swap of non-ASCII bounds must still parse as JSON");
        assert!(after.contains("'apple' AND 'über'"), "got: {after}");
    }

    #[test]
    fn huge_integer_between_beyond_i64_offers_no_swap() {
        // Integers that overflow i64 must NOT be compared via f64 (rounding
        // could mis-order) — conservative: no swap offered.
        let big = "99999999999999999999999999"; // > i64::MAX
        let src = format!(
            r#"{{"name":"u","columns":[{{"name":"n","type":"integer"}}],"constraints":[{{"type":"check","name":"chk","expr":"n BETWEEN {big} AND 0"}}]}}"#
        );
        let tree = parse(&src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(&src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Swap reversed BETWEEN bounds"),
            "i64-overflow integer bounds must not be ordered via f64; no swap expected"
        );
    }

    #[test]
    fn between_swap_inside_and_composition() {
        // Reversed BETWEEN nested in an AND composition is still fixable.
        let src = r#"{"name":"u","columns":[{"name":"age","type":"integer"}],"constraints":[{"type":"check","name":"chk","expr":"age > 0 AND age BETWEEN 150 AND 18"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let swap = actions
            .iter()
            .find(|a| a.title == "Swap reversed BETWEEN bounds")
            .expect("nested BETWEEN swap action missing");
        let after = apply(src, swap);
        assert!(
            after.contains("age > 0 AND age BETWEEN 18 AND 150"),
            "nested bounds must be swapped, got: {after}"
        );
    }

    #[rstest::rstest]
    #[case::missing_literal("age BETWEEN")]
    #[case::missing_and_keyword("age BETWEEN 5 OR 1")]
    fn malformed_between_shapes_offer_no_swap(#[case] expr: &str) {
        let src = format!(
            r#"{{"name":"u","columns":[{{"name":"age","type":"integer"}}],"constraints":[{{"type":"check","name":"chk","expr":"{expr}"}}]}}"#
        );
        let tree = parse(&src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(&src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Swap reversed BETWEEN bounds"),
            "malformed BETWEEN must not offer swap, got: {actions:?}"
        );
    }

    #[test]
    fn malformed_column_shapes_exercise_missing_value_branches() {
        for src in [
            r#"{"name":"u","columns":[{"name":"x"}]}"#,
            r#"{"name":"u","columns":[{"name":"x","type":}]}"#,
            r#"{"name":"u","columns":[{"name":"x","type":"text","default":}]}"#,
            r#"{"name":"u","columns":[{"name":"x","type":,"default":"'x'"}]}"#,
        ] {
            let tree = parse(src);
            let cursor = src.find('x').unwrap();
            let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
            assert!(
                actions.iter().all(|a| !a.title.is_empty()),
                "malformed/missing-value column shapes must be handled without panic and any offered action must have a title: {actions:?}"
            );
        }
    }

    fn with_enclosing_column<R>(
        src: &str,
        needle: &str,
        f: impl FnOnce(tree_sitter::Node<'_>, &[u8]) -> R,
    ) -> R {
        let tree = parse(src);
        let cursor = src.find(needle).unwrap();
        let column = enclosing_column_object(tree.root_node(), src.as_bytes(), cursor)
            .expect("cursor should sit inside a column object");
        f(column, src.as_bytes())
    }

    #[rstest::rstest]
    #[case::type_value_missing(r#"{"name":"u","columns":[{"name":"x","type":}]}"#)]
    #[case::type_value_missing_before_default(
        r#"{"name":"u","columns":[{"name":"x","type":,"default":"'pending'"}]}"#
    )]
    fn direct_type_conversion_skips_missing_type_values(#[case] src: &str) {
        with_enclosing_column(src, "type", |column, bytes| {
            assert!(type_conversions(column, bytes).is_empty());
        });
    }

    #[rstest::rstest]
    #[case::default_value_missing(
        r#"{"name":"u","columns":[{"name":"x","type":"text","default":}]}"#
    )]
    #[case::type_pair_missing(r#"{"name":"u","columns":[{"name":"x","default":"'pending'"}]}"#)]
    #[case::type_value_missing(
        r#"{"name":"u","columns":[{"name":"x","default":"'pending'","type":}]}"#
    )]
    fn direct_enum_extraction_skips_malformed_column_shapes(#[case] src: &str) {
        with_enclosing_column(src, "x", |column, bytes| {
            assert!(enum_extraction(column, bytes).is_empty());
        });
    }

    #[test]
    fn enclosing_column_object_descends_to_deep_cursor() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""integer""#).unwrap() + 2;
        let column = enclosing_column_object(tree.root_node(), src.as_bytes(), cursor)
            .expect("nested cursor should resolve to enclosing column");
        let name_pair = find_pair(column, src.as_bytes(), "name").expect("column name pair");
        let name_value = name_pair.named_child(1).expect("column name value");
        assert_eq!(&src[name_value.byte_range()], r#""id""#);
    }

    #[test]
    fn enum_extraction_skips_when_type_is_already_object() {
        let src = r#"{"name":"u","columns":[{"name":"status","type":{"kind":"enum","name":"s","values":["pending"]},"default":"'pending'"}]}"#;
        let tree = parse(src);
        let cursor = src.find("status").unwrap();
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        assert!(
            actions
                .iter()
                .all(|a| !a.title.starts_with("Extract default")),
            "object type should skip enum extraction, got: {actions:?}"
        );
    }

    #[test]
    fn false_flags_flip_to_true_and_weird_nullable_is_ignored() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","primary_key":false,"nullable":null}]}"#;
        let tree = parse(src);
        let cursor = src.find("id").unwrap();
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let pk = actions
            .iter()
            .find(|a| a.title == "Mark column as primary key")
            .expect("false primary_key should flip to true");
        assert_eq!(pk.edits[0].new_text, "true");
        assert!(
            actions
                .iter()
                .all(|a| a.title != "Allow NULL" && a.title != "Make column NOT NULL"),
            "nullable:null should not produce nullable toggle, got: {actions:?}"
        );
    }

    #[test]
    fn remove_pair_edit_handles_first_and_only_pair_shapes() {
        let first_src =
            r#"{"name":"u","columns":[{"primary_key":true,"name":"id","type":"integer"}]}"#;
        let first_tree = parse(first_src);
        let first_cursor = first_src.find("id").unwrap();
        let first_actions = compute(
            first_src,
            DocumentFormat::Json,
            Some(&first_tree),
            first_cursor..first_cursor,
        );
        let first = first_actions
            .iter()
            .find(|a| a.title == "Unmark primary key")
            .expect("remove first pair action");
        serde_json::from_str::<serde_json::Value>(&apply(first_src, first))
            .expect("removing first pair leaves valid JSON");
        let only_src = r#"{"name":"u","columns":[{"primary_key":true}]}"#;
        let only_tree = parse(only_src);
        let only_cursor = only_src.find("primary_key").unwrap();
        let only_actions = compute(
            only_src,
            DocumentFormat::Json,
            Some(&only_tree),
            only_cursor..only_cursor,
        );
        let only = only_actions
            .iter()
            .find(|a| a.title == "Unmark primary key")
            .expect("remove only pair action");
        let after = apply(only_src, only);
        assert!(
            after.contains("{}"),
            "single-pair removal should leave an empty object, got: {after}"
        );
    }

    #[test]
    fn float_literal_between_swap_transposes_bounds() {
        let src = r#"{"name":"u","columns":[{"name":"score","type":"real"}],"constraints":[{"type":"check","name":"chk","expr":"score BETWEEN 9.5 AND 1.5"}]}"#;
        let tree = parse(src);
        let cursor = src.find("BETWEEN").unwrap() + 2;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let swap = actions
            .iter()
            .find(|a| a.title == "Swap reversed BETWEEN bounds")
            .expect("float BETWEEN swap action");

        assert!(apply(src, swap).contains("score BETWEEN 1.5 AND 9.5"));
    }

    #[test]
    fn no_tree_returns_empty_actions() {
        assert!(compute("anything", DocumentFormat::Json, None, 0..0).is_empty());
    }

    #[rstest::rstest]
    #[case::unknown_simple_type(
        r#"{"name":"u","columns":[{"name":"active","type":"boolean"}]}"#,
        "active",
        "Convert to"
    )]
    #[case::default_non_string(
        r#"{"name":"u","columns":[{"name":"n","type":"integer","default":0}]}"#,
        "n",
        "Extract default"
    )]
    #[case::default_non_quote_string(
        r#"{"name":"u","columns":[{"name":"x","type":"text","default":"NOW()"}]}"#,
        "x",
        "Extract default"
    )]
    fn code_actions_do_not_offer_inapplicable_titles(
        #[case] src: &str,
        #[case] cursor_needle: &str,
        #[case] forbidden_title_prefix: &str,
    ) {
        let tree = parse(src);
        let cursor = src.find(cursor_needle).unwrap() + 1;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);

        assert!(
            actions
                .iter()
                .all(|a| !a.title.starts_with(forbidden_title_prefix)),
            "unexpected `{forbidden_title_prefix}` action in {actions:?}"
        );
    }

    #[test]
    fn nullable_true_offers_not_null_edit() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":true}]}"#;
        let tree = parse(src);
        let cursor = src.find(r#""id""#).unwrap() + 1;
        let actions = compute(src, DocumentFormat::Json, Some(&tree), cursor..cursor);
        let not_null = actions
            .iter()
            .find(|a| a.title == "Make column NOT NULL")
            .expect("nullable: true → false action");

        assert_eq!(not_null.edits[0].new_text, "false");
    }
}
