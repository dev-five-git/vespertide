//! Tree-sitter walk that classifies every interesting JSON node in a
//! Vespertide model file into a semantic token type / modifier set.
//!
//! Classification rules:
//!
//! | JSON shape                                | Token type / modifier             |
//! |-------------------------------------------|-----------------------------------|
//! | Top-level `"name": "X"` value             | `class` + `declaration`           |
//! | `columns[*].name` value                   | `property` + `declaration`        |
//! | `columns[*].type` simple string           | `type`                            |
//! | `columns[*].type.kind` value              | `enumMember`                      |
//! | `columns[*].type.values[*]` (string enum) | `enumMember`                      |
//! | `columns[*].type.values[*].name` (int enum) | `enumMember`                    |
//! | `foreign_key.ref_table` value             | `class` + `definition`            |
//! | `foreign_key.ref_columns[*]` entry        | `property` + `definition`         |
//! | `foreign_key.on_delete` / `on_update` value | `enumMember`                    |
//! | `default` value (string)                  | `string`                          |
//! | `default` literal `true`/`false`/`null`   | `keyword`                         |
//! | Any numeric literal                       | `number`                          |
//!
//! Inner content ranges (without surrounding `"`) are used so themes
//! highlight the identifier alone — quotes stay neutral and match the
//! rest of the JSON punctuation.

#![expect(
    clippy::struct_excessive_bools,
    reason = "semantic-token classifier context tracks independent JSON ancestor flags; collapsing them would obscure token rules"
)]

use super::{RawToken, check_expr_tokens, legend::ModIdx, legend::TokenIdx};

/// Classify the entire JSON document.
#[must_use]
pub fn classify(source: &str, tree: &tree_sitter::Tree) -> Vec<RawToken> {
    let mut out = Vec::new();
    let source_bytes = source.as_bytes();
    walk(tree.root_node(), source_bytes, Ctx::default(), &mut out);
    out
}

/// Cursor state passed down the recursion — tells the value-classifier
/// which key it's underneath so we can disambiguate same-shaped strings
/// (`"text"` in `"type"` vs `"text"` in `"comment"`).
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    inside_columns: bool,
    /// Owning column object — set when we recurse into a column body.
    inside_column: bool,
    /// `"type": { ... }` inner mapping.
    inside_complex_type_object: bool,
    /// `enum` values array — children are valid enum members.
    inside_enum_values_array: bool,
    /// `foreign_key` value object.
    inside_foreign_key: bool,
    /// `ref_columns` array under a `foreign_key`.
    inside_ref_columns: bool,
    /// `constraints` array — CHECK `expr` strings get SQL-ish tokenisation.
    inside_constraints: bool,
    /// Depth of object nesting we've already walked. The TOP-LEVEL
    /// object (the table itself) is depth 1.
    object_depth: u32,
}

fn walk(node: tree_sitter::Node<'_>, source: &[u8], ctx: Ctx, out: &mut Vec<RawToken>) {
    if node.kind() == "object" {
        let new_ctx = Ctx {
            object_depth: ctx.object_depth + 1,
            ..ctx
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, source, new_ctx, out);
        }
        return;
    }

    if node.kind() == "pair" {
        classify_pair(node, source, ctx, out);
        return;
    }

    // For other interior nodes (`array`, `document`, etc.) just recurse.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, source, ctx, out);
    }
}

fn classify_pair(pair: tree_sitter::Node<'_>, source: &[u8], ctx: Ctx, out: &mut Vec<RawToken>) {
    if let Some(key) = pair.named_child(0)
        && let Some(value) = pair.named_child(1)
        && let Some(key_text) = scalar_text(key, source)
    {
        match key_text {
            // Top-level table identifier — `object_depth == 1` distinguishes
            // it from nested `name` keys (column.name, enum member.name).
            "name" if ctx.object_depth == 1 && !ctx.inside_columns => {
                push_string_inner(value, TokenIdx::Class, ModIdx::Declaration as u32, out);
            }
            "name" if ctx.inside_column && !ctx.inside_complex_type_object => {
                push_string_inner(value, TokenIdx::Property, ModIdx::Declaration as u32, out);
            }
            "name" if ctx.inside_enum_values_array => {
                // integer-enum member `{"name":"low", "value":0}` — the name
                // is the enum-member identifier.
                push_string_inner(value, TokenIdx::EnumMember, 0, out);
            }
            "columns" => {
                let new_ctx = Ctx {
                    inside_columns: true,
                    ..ctx
                };
                recurse_into_value(value, source, new_ctx, out);
                return;
            }
            "constraints" => {
                let new_ctx = Ctx {
                    inside_constraints: true,
                    ..ctx
                };
                recurse_into_value(value, source, new_ctx, out);
                return;
            }
            "expr" if ctx.inside_constraints && value.kind() == "string" => {
                emit_json_check_expr_tokens(value, source, out);
                return;
            }
            "type" if ctx.inside_columns => {
                classify_type_value(value, source, ctx, out);
                return;
            }
            "kind" if ctx.inside_complex_type_object => {
                push_string_inner(value, TokenIdx::EnumMember, 0, out);
            }
            "values" if ctx.inside_complex_type_object => {
                let new_ctx = Ctx {
                    inside_enum_values_array: true,
                    ..ctx
                };
                recurse_into_value(value, source, new_ctx, out);
                return;
            }
            "foreign_key" => {
                let new_ctx = Ctx {
                    inside_foreign_key: true,
                    ..ctx
                };
                recurse_into_value(value, source, new_ctx, out);
                return;
            }
            "ref_table" if ctx.inside_foreign_key => {
                push_string_inner(value, TokenIdx::Class, ModIdx::Definition as u32, out);
            }
            "ref_columns" if ctx.inside_foreign_key => {
                let new_ctx = Ctx {
                    inside_ref_columns: true,
                    ..ctx
                };
                recurse_into_value(value, source, new_ctx, out);
                return;
            }
            "on_delete" | "on_update" if ctx.inside_foreign_key => {
                push_string_inner(value, TokenIdx::EnumMember, 0, out);
            }
            "default" if ctx.inside_column => {
                classify_default_value(value, out);
            }
            _ => {}
        }

        // Recurse into the value so we still surface nested literals.
        recurse_into_value(value, source, ctx, out);
    }
}

/// Recurse into the value of a pair, propagating context updates we may
/// have set right before this call. Specific contexts (`ref_columns`,
/// `enum_values_array`) are checked BEFORE the general columns-array
/// case because they are nested inside `columns` and would otherwise
/// be shadowed by the broader branch.
fn recurse_into_value(
    value: tree_sitter::Node<'_>,
    source: &[u8],
    ctx: Ctx,
    out: &mut Vec<RawToken>,
) {
    if value.kind() == "array" && ctx.inside_ref_columns {
        let mut cursor = value.walk();
        for child in value.children(&mut cursor) {
            if child.kind() == "string" {
                push_string_inner(child, TokenIdx::Property, ModIdx::Definition as u32, out);
            } else {
                walk(child, source, ctx, out);
            }
        }
        return;
    }

    if value.kind() == "array" && ctx.inside_enum_values_array {
        let mut cursor = value.walk();
        for child in value.children(&mut cursor) {
            if child.kind() == "string" {
                push_string_inner(child, TokenIdx::EnumMember, 0, out);
            } else {
                // integer-enum objects — recurse so the `name` key
                // emits its own enumMember token via the normal path.
                walk(child, source, ctx, out);
            }
        }
        return;
    }

    // `array` itself isn't an object — but if we're inside `columns`
    // each element is a column object: flip `inside_column` for them.
    if value.kind() == "array" && ctx.inside_columns {
        let mut cursor = value.walk();
        for child in value.children(&mut cursor) {
            let element_ctx = if child.kind() == "object" {
                Ctx {
                    inside_column: true,
                    ..ctx
                }
            } else {
                ctx
            };
            walk(child, source, element_ctx, out);
        }
        return;
    }

    walk(value, source, ctx, out);
}

fn classify_type_value(
    value: tree_sitter::Node<'_>,
    source: &[u8],
    ctx: Ctx,
    out: &mut Vec<RawToken>,
) {
    match value.kind() {
        "string" => {
            push_string_inner(value, TokenIdx::Type, 0, out);
        }
        "object" => {
            let inner_ctx = Ctx {
                inside_complex_type_object: true,
                ..ctx
            };
            // Counts as +1 object depth via the regular `object` arm.
            walk(value, source, inner_ctx, out);
        }
        _ => {
            walk(value, source, ctx, out);
        }
    }
}

fn classify_default_value(value: tree_sitter::Node<'_>, out: &mut Vec<RawToken>) {
    match value.kind() {
        "string" => push_string_inner(value, TokenIdx::String, 0, out),
        "true" | "false" | "null" => out.push(RawToken {
            byte_range: value.byte_range(),
            token_type: TokenIdx::Keyword as u32,
            token_modifiers: 0,
        }),
        "number" => out.push(RawToken {
            byte_range: value.byte_range(),
            token_type: TokenIdx::Number as u32,
            token_modifiers: 0,
        }),
        _ => {}
    }
}

fn emit_json_check_expr_tokens(
    string_node: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<RawToken>,
) {
    let raw = string_node.byte_range();
    if raw.end.saturating_sub(raw.start) >= 2 {
        let inner_start = raw.start + 1;
        let inner_end = raw.end - 1;
        if inner_end > inner_start
            && let Some(expr_text) = source
                .get(inner_start..inner_end)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            check_expr_tokens::emit_check_expr_tokens(expr_text, inner_start, out);
        }
    }
}

/// Emit a token covering only the INNER content of a JSON string node
/// (i.e. the bytes between the surrounding `"`). Drops zero-length
/// inner spans (empty strings) defensively — the encoder would discard
/// them anyway, but we save it the work.
fn push_string_inner(
    string_node: tree_sitter::Node<'_>,
    token_type: TokenIdx,
    token_modifiers: u32,
    out: &mut Vec<RawToken>,
) {
    if string_node.kind() != "string" {
        return;
    }
    let range = string_node.named_child(0).map_or_else(
        || {
            let r = string_node.byte_range();
            if r.end.saturating_sub(r.start) >= 2 {
                (r.start + 1)..(r.end - 1)
            } else {
                r
            }
        },
        |inner| inner.byte_range(),
    );
    if range.end <= range.start {
        return;
    }
    out.push(RawToken {
        byte_range: range,
        token_type: token_type as u32,
        token_modifiers,
    });
}

fn scalar_text<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let raw = std::str::from_utf8(source.get(node.byte_range())?).ok()?;
    Some(raw.trim().trim_matches('"').trim_matches('\''))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::test_support::*;
    use rstest::rstest;

    fn classify_src(src: &str) -> Vec<RawToken> {
        if src.is_empty() {
            return ParserPool::new()
                .parse(src, DocumentFormat::Json)
                .as_ref()
                .map_or_else(Vec::new, |tree| classify(src, tree));
        }
        let tree = parse_json(src);
        classify(src, &tree)
    }

    fn types_present(tokens: &[RawToken]) -> Vec<u32> {
        let mut types: Vec<u32> = tokens.iter().map(|t| t.token_type).collect();
        types.sort_unstable();
        types.dedup();
        types
    }

    #[test]
    fn top_level_name_becomes_class_declaration() {
        let src = r#"{"name":"user","columns":[]}"#;
        let tokens = classify_src(src);
        let name_start = src.find(r#""name":"user""#).unwrap() + 8;
        let class_tok = tokens
            .iter()
            .find(|t| t.byte_range.start == name_start)
            .expect("class token at user");
        assert_eq!(class_tok.token_type, TokenIdx::Class as u32);
        assert_eq!(class_tok.token_modifiers, ModIdx::Declaration as u32);
    }

    #[test]
    fn column_name_becomes_property_declaration() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tokens = classify_src(src);
        let id_start = src.find(r#""name":"id""#).unwrap() + 8;
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == id_start)
            .expect("column name token");
        assert_eq!(tok.token_type, TokenIdx::Property as u32);
        assert_eq!(tok.token_modifiers, ModIdx::Declaration as u32);
    }

    #[test]
    fn simple_type_becomes_type_token() {
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer"}]}"#;
        let tokens = classify_src(src);
        let integer_start = src.find(r#""type":"integer""#).unwrap() + 8;
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == integer_start)
            .expect("type token");
        assert_eq!(tok.token_type, TokenIdx::Type as u32);
    }

    #[test]
    fn complex_type_kind_value_is_enum_member() {
        let src = r#"{"name":"u","columns":[{"name":"t","type":{"kind":"varchar","length":255}}]}"#;
        let tokens = classify_src(src);
        let kind_start = src.find(r#""kind":"varchar""#).unwrap() + 8;
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == kind_start)
            .expect("kind token");
        assert_eq!(tok.token_type, TokenIdx::EnumMember as u32);
    }

    #[test]
    fn enum_string_values_are_enum_members() {
        let src = r#"{"name":"u","columns":[{"name":"s","type":{"kind":"enum","name":"st","values":["active","banned"]}}]}"#;
        let tokens = classify_src(src);
        let active_start = src.find(r#""active""#).unwrap() + 1;
        let banned_start = src.find(r#""banned""#).unwrap() + 1;
        let active = tokens
            .iter()
            .find(|t| t.byte_range.start == active_start)
            .expect("active");
        let banned = tokens
            .iter()
            .find(|t| t.byte_range.start == banned_start)
            .expect("banned");
        assert_eq!(active.token_type, TokenIdx::EnumMember as u32);
        assert_eq!(banned.token_type, TokenIdx::EnumMember as u32);
    }

    #[test]
    fn ref_table_value_is_class_definition() {
        let src = r#"{"name":"p","columns":[{"name":"a","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let tokens = classify_src(src);
        let user_start = src.find(r#""ref_table":"user""#).unwrap() + 13;
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == user_start)
            .expect("ref_table token");
        assert_eq!(tok.token_type, TokenIdx::Class as u32);
        assert_eq!(tok.token_modifiers, ModIdx::Definition as u32);
    }

    #[test]
    fn ref_columns_entries_are_property_definition() {
        let src = r#"{"name":"p","columns":[{"name":"a","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}]}"#;
        let tokens = classify_src(src);
        let id_start = src.find(r#"["id"]"#).unwrap() + 2;
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == id_start)
            .expect("ref_columns id token");
        assert_eq!(tok.token_type, TokenIdx::Property as u32);
        assert_eq!(tok.token_modifiers, ModIdx::Definition as u32);
    }

    #[test]
    fn on_delete_action_is_enum_member() {
        let src = r#"{"name":"p","columns":[{"name":"a","type":"integer","foreign_key":{"ref_table":"u","ref_columns":["id"],"on_delete":"cascade"}}]}"#;
        let tokens = classify_src(src);
        let action_start = src.find(r#""on_delete":"cascade""#).unwrap() + 13;
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == action_start)
            .expect("on_delete token");
        assert_eq!(tok.token_type, TokenIdx::EnumMember as u32);
    }

    #[test]
    fn default_keyword_literals_are_keyword_tokens() {
        let src = r#"{"name":"u","columns":[{"name":"x","type":"boolean","default":true}]}"#;
        let tokens = classify_src(src);
        let true_start = src.find(r#""default":true"#).unwrap() + 10;
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == true_start)
            .expect("keyword token");
        assert_eq!(tok.token_type, TokenIdx::Keyword as u32);
    }

    #[rstest]
    #[case::true_literal(
        r#"{"name":"u","columns":[{"name":"x","type":"boolean","default":true}]}"#,
        r#""default":true"#,
        10,
        Some(TokenIdx::Keyword)
    )]
    #[case::false_literal(
        r#"{"name":"u","columns":[{"name":"x","type":"boolean","default":false}]}"#,
        r#""default":false"#,
        10,
        Some(TokenIdx::Keyword)
    )]
    #[case::null_literal(
        r#"{"name":"u","columns":[{"name":"x","type":"integer","default":null}]}"#,
        r#""default":null"#,
        10,
        Some(TokenIdx::Keyword)
    )]
    #[case::number_literal(
        r#"{"name":"u","columns":[{"name":"x","type":"integer","default":42}]}"#,
        r#""default":42"#,
        10,
        Some(TokenIdx::Number)
    )]
    #[case::string_literal(
        r#"{"name":"u","columns":[{"name":"x","type":"text","default":"abc"}]}"#,
        r#""default":"abc""#,
        11,
        Some(TokenIdx::String)
    )]
    #[case::array_literal(
        r#"{"name":"u","columns":[{"name":"x","type":"integer","default":[]}]}"#,
        r#""default":[]"#,
        10,
        None
    )]
    fn json_default_literal_cases(
        #[case] src: &str,
        #[case] needle: &str,
        #[case] offset: usize,
        #[case] expected_type: Option<TokenIdx>,
    ) {
        let tokens = classify_src(src);
        let start = src.find(needle).unwrap() + offset;
        let token = tokens.iter().find(|token| token.byte_range.start == start);

        if let Some(token_type) = expected_type {
            assert_eq!(
                token.expect("default literal token").token_type,
                token_type as u32
            );
        } else {
            assert!(
                token.is_none(),
                "no token expected at array default literal"
            );
        }
    }

    #[rstest]
    #[case::complex_type_object(
        r#"{"name":"u","columns":[{"name":"x","type":{"kind":"varchar","length":255}}]}"#,
        true
    )]
    #[case::array_type_value(r#"{"name":"u","columns":[{"name":"x","type":[1,2]}]}"#, false)]
    fn json_type_value_cases(#[case] src: &str, #[case] expected_enum_member: bool) {
        let tokens = classify_src(src);
        assert_eq!(
            tokens
                .iter()
                .any(|token| token.token_type == TokenIdx::EnumMember as u32),
            expected_enum_member
        );
    }

    #[rstest]
    #[case::array_at_document_root(r#"[{"name":"x"}]"#, None)]
    #[case::pair_with_missing_value(r#"{"name":}"#, None)]
    #[case::top_level_name(r#"{"name":"top_user"}"#, Some((r#""name":"top_user""#, 8, TokenIdx::Class, ModIdx::Declaration as u32)))]
    #[case::columns_array_non_object(r#"{"name":"u","columns":[42]}"#, None)]
    #[case::columns_before_name(r#"{"columns":[],"name":"second_name"}"#, Some((r#""name":"second_name""#, 8, TokenIdx::Class, ModIdx::Declaration as u32)))]
    fn json_walk_cases(
        #[case] src: &str,
        #[case] expected_token: Option<(&str, usize, TokenIdx, u32)>,
    ) {
        let tokens = classify_src(src);

        if let Some((needle, offset, token_type, token_modifiers)) = expected_token {
            let start = src.find(needle).unwrap() + offset;
            let token = tokens
                .iter()
                .find(|token| token.byte_range.start == start)
                .expect("expected semantic token");
            assert_eq!(token.token_type, token_type as u32);
            assert_eq!(token.token_modifiers, token_modifiers);
        }
    }

    #[test]
    fn json_classifier_on_empty_document_emits_no_tokens() {
        assert!(classify_src("").is_empty());
    }

    #[test]
    fn types_are_classified_correctly_in_a_realistic_doc() {
        let src = r#"{
            "name": "post",
            "columns": [
                {"name":"id","type":"integer","primary_key":true},
                {"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}
            ]
        }"#;
        let tokens = classify_src(src);
        let present = types_present(&tokens);
        // Must surface ALL of class, property, type, and the FK reference.
        assert!(present.contains(&(TokenIdx::Class as u32)));
        assert!(present.contains(&(TokenIdx::Property as u32)));
        assert!(present.contains(&(TokenIdx::Type as u32)));
    }

    #[test]
    fn check_expr_tokens_emitted_for_simple_compare_and_composition() {
        let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age_range","expr":"age > 0 AND age < 150"}]}"#;
        let tokens = classify_src(src);
        let expr_start = src.find("age > 0").expect("expr start present");
        let expr_end = src.find(r#"150""#).expect("expr end present") + 3;
        let check_tokens: Vec<&RawToken> = tokens
            .iter()
            .filter(|t| t.byte_range.start >= expr_start && t.byte_range.end <= expr_end + 1)
            .collect();

        assert_eq!(
            check_tokens.len(),
            7,
            "got {} check tokens: {:?}",
            check_tokens.len(),
            check_tokens
        );

        let types: Vec<u32> = check_tokens.iter().map(|t| t.token_type).collect();
        assert_eq!(
            types,
            vec![
                TokenIdx::Property as u32,
                TokenIdx::Keyword as u32,
                TokenIdx::Number as u32,
                TokenIdx::Keyword as u32,
                TokenIdx::Property as u32,
                TokenIdx::Keyword as u32,
                TokenIdx::Number as u32,
            ]
        );
        assert!(
            check_tokens.iter().all(|t| t.token_modifiers == 0),
            "CHECK expression tokens must not carry declaration/definition modifiers: {check_tokens:?}"
        );
    }

    #[test]
    fn multiple_check_constraints_emit_tokens_independently() {
        let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false},{"name":"height","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"},{"type":"check","name":"chk_height","expr":"height < 250"}]}"#;
        let tokens = classify_src(src);
        let expr_ranges = [
            {
                let start = src.find("age > 0").expect("age expr present");
                start..start + "age > 0".len()
            },
            {
                let start = src.find("height < 250").expect("height expr present");
                start..start + "height < 250".len()
            },
        ];

        let check_tokens: Vec<&RawToken> = tokens
            .iter()
            .filter(|t| {
                expr_ranges
                    .iter()
                    .any(|range| t.byte_range.start >= range.start && t.byte_range.end <= range.end)
            })
            .collect();

        assert_eq!(
            check_tokens.len(),
            6,
            "got {} check tokens: {:?}",
            check_tokens.len(),
            check_tokens
        );
        assert_eq!(
            check_tokens
                .iter()
                .filter(|t| t.token_type == TokenIdx::Property as u32)
                .count(),
            2,
            "one column token should be emitted per CHECK expression: {check_tokens:?}"
        );
        assert_eq!(
            check_tokens
                .iter()
                .filter(|t| t.token_type == TokenIdx::Number as u32)
                .count(),
            2,
            "one numeric literal token should be emitted per CHECK expression: {check_tokens:?}"
        );
    }

    #[test]
    fn non_constraint_tokens_unchanged_when_check_added() {
        let src = r#"{
            "name": "post",
            "columns": [
                {"name":"id","type":"integer","primary_key":true},
                {"name":"author_id","type":"integer","foreign_key":{"ref_table":"user","ref_columns":["id"]}}
            ],
            "constraints": [
                {"type":"check","name":"chk_author_positive","expr":"author_id > 0"}
            ]
        }"#;
        let tokens = classify_src(src);

        let post_start = src
            .find(r#""name": "post""#)
            .expect("top-level name present")
            + 9;
        let post = tokens
            .iter()
            .find(|t| t.byte_range.start == post_start)
            .expect("token not found at byte offset for top-level table name 'post'");
        assert_eq!(post.token_type, TokenIdx::Class as u32);
        assert_eq!(post.token_modifiers, ModIdx::Declaration as u32);

        for column in ["id", "author_id"] {
            let needle = format!(r#""name":"{column}""#);
            let start = src.find(&needle).expect("column name present") + 8;
            let token = tokens
                .iter()
                .find(|t| t.byte_range.start == start)
                .expect("token not found at byte offset for column name");
            assert_eq!(token.token_type, TokenIdx::Property as u32);
            assert_eq!(token.token_modifiers, ModIdx::Declaration as u32);
        }

        let integer_tokens = src
            .match_indices(r#""type":"integer""#)
            .map(|(idx, _)| idx + 8);
        for start in integer_tokens {
            let token = tokens
                .iter()
                .find(|t| t.byte_range.start == start)
                .expect("token not found at byte offset for simple type 'integer'");
            assert_eq!(token.token_type, TokenIdx::Type as u32);
            assert_eq!(token.token_modifiers, 0);
        }

        let user_start = src
            .find(r#""ref_table":"user""#)
            .expect("ref_table present")
            + 13;
        let user = tokens
            .iter()
            .find(|t| t.byte_range.start == user_start)
            .expect("token not found at byte offset for ref_table value 'user'");
        assert_eq!(user.token_type, TokenIdx::Class as u32);
        assert_eq!(user.token_modifiers, ModIdx::Definition as u32);

        let ref_id_start = src
            .find(r#""ref_columns":["id"]"#)
            .expect("ref_columns present")
            + 16;
        let ref_id = tokens
            .iter()
            .find(|t| t.byte_range.start == ref_id_start)
            .expect("token not found at byte offset for ref_columns value 'id'");
        assert_eq!(ref_id.token_type, TokenIdx::Property as u32);
        assert_eq!(ref_id.token_modifiers, ModIdx::Definition as u32);

        let expr_start = src.find("author_id > 0").expect("CHECK expr present");
        let expr_end = expr_start + "author_id > 0".len();
        let check_tokens: Vec<&RawToken> = tokens
            .iter()
            .filter(|t| t.byte_range.start >= expr_start && t.byte_range.end <= expr_end)
            .collect();
        assert_eq!(
            check_tokens.len(),
            3,
            "CHECK expression tokens should be additive and not replace existing tokens: {check_tokens:?}"
        );
    }

    fn first_node<'tree>(
        node: tree_sitter::Node<'tree>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'tree>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = first_node(child, kind) {
                return Some(found);
            }
        }
        None
    }

    fn first_pair<'tree>(tree: &'tree tree_sitter::Tree, src: &str) -> tree_sitter::Node<'tree> {
        first_node(tree.root_node(), "pair").unwrap_or_else(|| panic!("pair missing in {src}"))
    }

    #[test]
    fn classify_pair_early_returns_for_missing_key_value_and_bad_key_text() {
        let root_src = r#"{"name":"u"}"#;
        let tree = ParserPool::new()
            .parse(root_src, DocumentFormat::Json)
            .unwrap();
        let mut out = Vec::new();
        classify_pair(
            tree.root_node(),
            root_src.as_bytes(),
            Ctx::default(),
            &mut out,
        );

        let missing_value = r#"{"name":}"#;
        let tree = ParserPool::new()
            .parse(missing_value, DocumentFormat::Json)
            .unwrap();
        classify_pair(
            first_pair(&tree, missing_value),
            missing_value.as_bytes(),
            Ctx::default(),
            &mut out,
        );

        let valid = r#"{"name":"u"}"#;
        let tree = ParserPool::new()
            .parse(valid, DocumentFormat::Json)
            .unwrap();
        let pair = first_pair(&tree, valid);
        let mut bad = valid.as_bytes().to_vec();
        let key_start = valid.find("name").unwrap();
        bad[key_start] = 0xff;
        classify_pair(pair, &bad, Ctx::default(), &mut out);

        assert!(out.is_empty());
    }

    #[test]
    fn integer_enum_member_name_is_classified() {
        let src = r#"{"name":"u","columns":[{"name":"priority","type":{"kind":"enum","name":"priority_level","values":[{"name":"low","value":0}]}}]}"#;
        let tokens = classify_src(src);
        let low_start = src.find(r#""name":"low""#).unwrap() + 8;
        let tok = tokens
            .iter()
            .find(|token| token.byte_range.start == low_start)
            .expect("integer enum member name token");

        assert_eq!(tok.token_type, TokenIdx::EnumMember as u32);
    }

    #[test]
    fn check_expr_emitter_defensively_returns_for_invalid_ranges_and_bytes() {
        let punct_src = r#"{"name":"u"}"#;
        let tree = ParserPool::new()
            .parse(punct_src, DocumentFormat::Json)
            .unwrap();
        let punctuation = tree.root_node().child(0).unwrap();
        let mut out = Vec::new();
        emit_json_check_expr_tokens(punctuation, punct_src.as_bytes(), &mut out);

        let empty = r#"{"expr":""}"#;
        let tree = ParserPool::new()
            .parse(empty, DocumentFormat::Json)
            .unwrap();
        let string_node = first_pair(&tree, empty).named_child(1).unwrap();
        emit_json_check_expr_tokens(string_node, empty.as_bytes(), &mut out);
        emit_json_check_expr_tokens(string_node, b"", &mut out);

        let valid = r#"{"expr":"age"}"#;
        let tree = ParserPool::new()
            .parse(valid, DocumentFormat::Json)
            .unwrap();
        let string_node = first_pair(&tree, valid).named_child(1).unwrap();
        emit_json_check_expr_tokens(string_node, b"{}", &mut out);
        let mut bad = valid.as_bytes().to_vec();
        let age = valid.find("age").unwrap();
        bad[age] = 0xff;
        emit_json_check_expr_tokens(string_node, &bad, &mut out);

        assert!(out.is_empty());
    }

    #[test]
    fn empty_json_string_value_does_not_emit_zero_length_token() {
        let tokens = classify_src(r#"{"name":""}"#);

        assert!(tokens.is_empty());
    }
}
