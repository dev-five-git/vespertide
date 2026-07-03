//! YAML counterpart to [`super::classify_json`]. Same semantic mapping,
//! but the YAML tree-sitter grammar uses different node kinds:
//!
//! | tree-sitter-yaml          | role                                   |
//! |---------------------------|----------------------------------------|
//! | `block_mapping`           | top-level mapping (the table)          |
//! | `block_mapping_pair`      | a `key: value` pair                    |
//! | `flow_node`/`block_node`  | pure wrappers — peeled                 |
//! | `block_sequence`          | YAML list                              |
//! | `block_sequence_item`     | `- ...` entry                          |
//! | `flow_sequence`           | `[a, b, c]` inline list                |
//! | `flow_mapping`            | `{a: b, c: d}` inline mapping          |
//! | `plain_scalar`            | unquoted scalar `varchar`              |
//! | `double_quote_scalar` /   | quoted scalar — trim 1 byte each side  |
//! | `single_quote_scalar`     |                                        |
//!
//! YAML quoted scalars include the delimiters in their byte range,
//! plain scalars don't. The push helpers trim where appropriate.

#![expect(
    clippy::struct_excessive_bools,
    reason = "semantic-token classifier context tracks independent YAML ancestor flags; collapsing them would obscure token rules"
)]

use super::{RawToken, check_expr_tokens, legend::ModIdx, legend::TokenIdx};

#[must_use]
pub fn classify(source: &str, tree: &tree_sitter::Tree) -> Vec<RawToken> {
    let mut out = Vec::new();
    walk(
        tree.root_node(),
        source.as_bytes(),
        Ctx::default(),
        &mut out,
    );
    out
}

#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    inside_columns: bool,
    inside_column: bool,
    inside_complex_type_object: bool,
    inside_enum_values_array: bool,
    inside_foreign_key: bool,
    inside_ref_columns: bool,
    inside_constraints: bool,
    mapping_depth: u32,
}

fn walk(node: tree_sitter::Node<'_>, source: &[u8], ctx: Ctx, out: &mut Vec<RawToken>) {
    match node.kind() {
        "block_mapping" | "flow_mapping" => {
            let new_ctx = Ctx {
                mapping_depth: ctx.mapping_depth + 1,
                ..ctx
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, new_ctx, out);
            }
        }
        "block_mapping_pair" | "flow_pair" => {
            classify_pair(node, source, ctx, out);
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, source, ctx, out);
            }
        }
    }
}

fn classify_pair(pair: tree_sitter::Node<'_>, source: &[u8], ctx: Ctx, out: &mut Vec<RawToken>) {
    if let Some(key_node) = pair.named_child(0)
        && let Some(value_node) = pair.named_child(1)
    {
        let key_node = unwrap_yaml(key_node);
        let value_node = unwrap_yaml(value_node);
        if let Some(key_text) = scalar_text(key_node, source) {
            match key_text {
                "name" if ctx.mapping_depth == 1 && !ctx.inside_columns => {
                    push_scalar(value_node, TokenIdx::Class, ModIdx::Declaration as u32, out);
                }
                "name" if ctx.inside_column && !ctx.inside_complex_type_object => {
                    push_scalar(
                        value_node,
                        TokenIdx::Property,
                        ModIdx::Declaration as u32,
                        out,
                    );
                }
                "name" if ctx.inside_enum_values_array => {
                    push_scalar(value_node, TokenIdx::EnumMember, 0, out);
                }
                "columns" => {
                    let new_ctx = Ctx {
                        inside_columns: true,
                        ..ctx
                    };
                    recurse_value(value_node, source, new_ctx, out);
                    return;
                }
                "constraints" => {
                    let new_ctx = Ctx {
                        inside_constraints: true,
                        ..ctx
                    };
                    recurse_value(value_node, source, new_ctx, out);
                    return;
                }
                "expr" if ctx.inside_constraints => {
                    emit_yaml_check_expr_tokens(value_node, source, out);
                    return;
                }
                "type" if ctx.inside_columns => {
                    classify_type_value(value_node, source, ctx, out);
                    return;
                }
                "kind" if ctx.inside_complex_type_object => {
                    push_scalar(value_node, TokenIdx::EnumMember, 0, out);
                }
                "values" if ctx.inside_complex_type_object => {
                    let new_ctx = Ctx {
                        inside_enum_values_array: true,
                        ..ctx
                    };
                    recurse_value(value_node, source, new_ctx, out);
                    return;
                }
                "foreign_key" => {
                    let new_ctx = Ctx {
                        inside_foreign_key: true,
                        ..ctx
                    };
                    recurse_value(value_node, source, new_ctx, out);
                    return;
                }
                "ref_table" if ctx.inside_foreign_key => {
                    push_scalar(value_node, TokenIdx::Class, ModIdx::Definition as u32, out);
                }
                "ref_columns" if ctx.inside_foreign_key => {
                    let new_ctx = Ctx {
                        inside_ref_columns: true,
                        ..ctx
                    };
                    recurse_value(value_node, source, new_ctx, out);
                    return;
                }
                "on_delete" | "on_update" if ctx.inside_foreign_key => {
                    push_scalar(value_node, TokenIdx::EnumMember, 0, out);
                }
                "default" if ctx.inside_column => {
                    classify_default(value_node, out);
                }
                _ => {}
            }

            recurse_value(value_node, source, ctx, out);
        }
    }
}

fn recurse_value(value: tree_sitter::Node<'_>, source: &[u8], ctx: Ctx, out: &mut Vec<RawToken>) {
    let v = unwrap_yaml(value);

    if ctx.inside_ref_columns && matches!(v.kind(), "block_sequence" | "flow_sequence") {
        let mut cursor = v.walk();
        for child in v.children(&mut cursor) {
            let element = unwrap_yaml(child);
            if is_scalar(element.kind()) {
                push_scalar(element, TokenIdx::Property, ModIdx::Definition as u32, out);
            } else {
                walk(child, source, ctx, out);
            }
        }
        return;
    }

    if ctx.inside_enum_values_array && matches!(v.kind(), "block_sequence" | "flow_sequence") {
        let mut cursor = v.walk();
        for child in v.children(&mut cursor) {
            let element = unwrap_yaml(child);
            if is_scalar(element.kind()) {
                push_scalar(element, TokenIdx::EnumMember, 0, out);
            } else {
                walk(child, source, ctx, out);
            }
        }
        return;
    }

    // The general "columns array" case must come last — nested arrays
    // (ref_columns, values) sit *inside* columns and would otherwise
    // be shadowed by this broader branch.
    if ctx.inside_columns && matches!(v.kind(), "block_sequence" | "flow_sequence") {
        let mut cursor = v.walk();
        for child in v.children(&mut cursor) {
            let element = unwrap_yaml(child);
            let item_ctx = if matches!(
                element.kind(),
                "block_mapping" | "flow_mapping" | "block_sequence_item"
            ) {
                Ctx {
                    inside_column: true,
                    ..ctx
                }
            } else {
                ctx
            };
            walk(child, source, item_ctx, out);
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
    let v = unwrap_yaml(value);
    match v.kind() {
        k if is_scalar(k) => {
            push_scalar(v, TokenIdx::Type, 0, out);
        }
        "block_mapping" | "flow_mapping" => {
            let inner_ctx = Ctx {
                inside_complex_type_object: true,
                ..ctx
            };
            walk(value, source, inner_ctx, out);
        }
        _ => walk(value, source, ctx, out),
    }
}

fn classify_default(value: tree_sitter::Node<'_>, out: &mut Vec<RawToken>) {
    let v = unwrap_yaml(value);
    if is_scalar(v.kind()) {
        // YAML doesn't distinguish bool/null/string/number scalars by
        // tree-sitter node kind alone — treat every scalar default as a
        // string; themes can still highlight `true`/`false`/`null` via
        // their own keyword scope from the syntax grammar.
        push_scalar(v, TokenIdx::String, 0, out);
    }
}

fn emit_yaml_check_expr_tokens(
    value: tree_sitter::Node<'_>,
    source: &[u8],
    out: &mut Vec<RawToken>,
) {
    let scalar = unwrap_yaml(value);
    if let Some(range) = check_expr_scalar_range(scalar)
        && range.end > range.start
        && let Some(expr_text) = source
            .get(range.clone())
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    {
        check_expr_tokens::emit_check_expr_tokens(expr_text, range.start, out);
    }
}

fn check_expr_scalar_range(node: tree_sitter::Node<'_>) -> Option<std::ops::Range<usize>> {
    crate::check_expr_range::expr_inner_range(node)
}

fn push_scalar(
    node: tree_sitter::Node<'_>,
    token_type: TokenIdx,
    token_modifiers: u32,
    out: &mut Vec<RawToken>,
) {
    let range = inner_scalar_range(node);
    if range.end <= range.start {
        return;
    }
    out.push(RawToken {
        byte_range: range,
        token_type: token_type as u32,
        token_modifiers,
    });
}

fn inner_scalar_range(node: tree_sitter::Node<'_>) -> std::ops::Range<usize> {
    let raw = node.byte_range();
    match node.kind() {
        "double_quote_scalar" | "single_quote_scalar" if raw.end.saturating_sub(raw.start) >= 2 => {
            (raw.start + 1)..(raw.end - 1)
        }
        _ => raw,
    }
}

fn unwrap_yaml(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    // Fused while-let so the empty-wrapper case (no usable `named_child(0)`)
    // and the kind-mismatch case share the same loop exit — no defensive
    // `return` line that only an (unobservable) empty tree-sitter wrapper
    // could reach.
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node")
        && let Some(inner) = current
            .named_child(0)
            .filter(|inner| inner.id() != current.id())
    {
        current = inner;
    }
    current
}

fn is_scalar(kind: &str) -> bool {
    matches!(
        kind,
        "plain_scalar"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "integer_scalar"
            | "float_scalar"
            | "boolean_scalar"
            | "null_scalar"
    )
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

    fn classify_yaml(src: &str) -> Vec<RawToken> {
        if src.is_empty() {
            return ParserPool::new()
                .parse(src, DocumentFormat::Yaml)
                .as_ref()
                .map_or_else(Vec::new, |tree| classify(src, tree));
        }
        let tree = parse_yaml(src);
        classify(src, &tree)
    }

    fn assert_check_expr_tokens(src: &str, expr: &str, expected: &[(&str, TokenIdx)]) {
        let tree = ParserPool::new()
            .parse(src, DocumentFormat::Yaml)
            .expect("parse");
        let tokens = classify(src, &tree);
        let expr_start = src.find(expr).expect("CHECK expr present");
        let expr_end = expr_start + expr.len();
        let check_tokens: Vec<&RawToken> = tokens
            .iter()
            .filter(|t| t.byte_range.start >= expr_start && t.byte_range.end <= expr_end)
            .collect();

        assert_eq!(
            check_tokens.len(),
            expected.len(),
            "got {} check tokens: {:?}",
            check_tokens.len(),
            check_tokens
        );

        let actual: Vec<(&str, u32)> = check_tokens
            .iter()
            .map(|token| (&src[token.byte_range.clone()], token.token_type))
            .collect();
        let expected: Vec<(&str, u32)> = expected
            .iter()
            .map(|(text, token_type)| (*text, *token_type as u32))
            .collect();
        assert_eq!(actual, expected);
        assert!(
            check_tokens.iter().all(|t| t.token_modifiers == 0),
            "CHECK expression tokens must not carry declaration/definition modifiers: {check_tokens:?}"
        );
    }

    #[derive(Debug, Clone, Copy)]
    enum YamlExpectation {
        TokenAt {
            needle: &'static str,
            offset: usize,
            token_type: TokenIdx,
            token_modifiers: u32,
        },
        HasEnumMember,
        EnumMembers(&'static [&'static str]),
        RefColumns(&'static [&'static str]),
        Empty,
        NoPanic,
    }

    #[rstest]
    #[case::simple_type("name: u\ncolumns:\n  - name: x\n    type: integer\n", YamlExpectation::TokenAt { needle: "type: integer", offset: 6, token_type: TokenIdx::Type, token_modifiers: 0 })]
    #[case::complex_type(
        "name: u\ncolumns:\n  - name: x\n    type:\n      kind: varchar\n      length: 255\n",
        YamlExpectation::HasEnumMember
    )]
    #[case::default_scalar("name: u\ncolumns:\n  - name: x\n    type: text\n    default: \"hello\"\n", YamlExpectation::TokenAt { needle: "default: \"hello\"", offset: 10, token_type: TokenIdx::String, token_modifiers: 0 })]
    #[case::foreign_key_on_delete("name: p\ncolumns:\n  - name: a\n    type: integer\n    foreign_key:\n      ref_table: u\n      ref_columns: [id]\n      on_delete: cascade\n", YamlExpectation::TokenAt { needle: "on_delete: cascade", offset: 11, token_type: TokenIdx::EnumMember, token_modifiers: 0 })]
    #[case::columns_non_mapping_item("name: u\ncolumns:\n  - foo\n", YamlExpectation::NoPanic)]
    #[case::enum_values("name: u\ncolumns:\n  - name: s\n    type:\n      kind: enum\n      name: st\n      values: [active, banned]\n", YamlExpectation::EnumMembers(&["active", "banned"]))]
    #[case::ref_columns_flow("name: p\ncolumns:\n  - name: a\n    type: integer\n    foreign_key:\n      ref_table: u\n      ref_columns: [id, email]\n", YamlExpectation::RefColumns(&["id", "email"]))]
    #[case::empty_document("", YamlExpectation::Empty)]
    fn yaml_semantic_token_cases(#[case] src: &str, #[case] expected: YamlExpectation) {
        let tokens = classify_yaml(src);

        match expected {
            YamlExpectation::TokenAt {
                needle,
                offset,
                token_type,
                token_modifiers,
            } => {
                let start = src.find(needle).unwrap() + offset;
                let token = tokens
                    .iter()
                    .find(|token| token.byte_range.start == start)
                    .expect("expected YAML semantic token");
                assert_eq!(token.token_type, token_type as u32);
                assert_eq!(token.token_modifiers, token_modifiers);
            }
            YamlExpectation::HasEnumMember => {
                assert!(
                    tokens
                        .iter()
                        .any(|token| token.token_type == TokenIdx::EnumMember as u32)
                );
            }
            YamlExpectation::EnumMembers(expected_members) => {
                let enum_members: Vec<&str> = tokens
                    .iter()
                    .filter(|token| token.token_type == TokenIdx::EnumMember as u32)
                    .map(|token| &src[token.byte_range.clone()])
                    .collect();
                for expected_member in expected_members {
                    assert!(
                        enum_members
                            .iter()
                            .any(|member| member.contains(expected_member)),
                        "EnumMember tokens missing `{expected_member}`, got: {enum_members:?}"
                    );
                }
            }
            YamlExpectation::RefColumns(expected_columns) => {
                let ref_columns: Vec<&str> = tokens
                    .iter()
                    .filter(|token| {
                        token.token_type == TokenIdx::Property as u32
                            && token.token_modifiers == ModIdx::Definition as u32
                    })
                    .map(|token| &src[token.byte_range.clone()])
                    .collect();
                for expected_column in expected_columns {
                    assert!(
                        ref_columns
                            .iter()
                            .any(|column| column.contains(expected_column)),
                        "ref_columns Property+Definition missing `{expected_column}`, got: {ref_columns:?}"
                    );
                }
            }
            YamlExpectation::Empty => assert!(tokens.is_empty()),
            YamlExpectation::NoPanic => {}
        }
    }

    #[test]
    fn yaml_table_and_column_names_classified() {
        let src = "name: user\ncolumns:\n  - name: id\n    type: integer\n";
        let tokens = classify_yaml(src);
        let user_start = src.find("user").unwrap();
        let id_start = src.find("name: id").unwrap() + 6;

        let user_tok = tokens
            .iter()
            .find(|t| t.byte_range.start == user_start)
            .expect("user");
        let id_tok = tokens
            .iter()
            .find(|t| t.byte_range.start == id_start)
            .expect("id");
        assert_eq!(user_tok.token_type, TokenIdx::Class as u32);
        assert_eq!(id_tok.token_type, TokenIdx::Property as u32);
    }

    #[test]
    fn yaml_foreign_key_ref_table_is_class_definition() {
        let src = "name: post\ncolumns:\n  - name: a\n    type: integer\n    foreign_key:\n      ref_table: user\n      ref_columns: [id]\n";
        let tokens = classify_yaml(src);
        let user_start = src.find("ref_table: user").unwrap() + "ref_table: ".len();
        let tok = tokens
            .iter()
            .find(|t| t.byte_range.start == user_start)
            .expect("ref_table token");
        assert_eq!(tok.token_type, TokenIdx::Class as u32);
        assert_eq!(tok.token_modifiers, ModIdx::Definition as u32);
    }

    #[test]
    fn bs_s1_literal_block_scalar_emits_check_tokens() {
        let src = "name: users\ncolumns:\n  - {name: id, type: integer, nullable: false, primary_key: true}\n  - {name: age, type: integer, nullable: false}\nconstraints:\n  - type: check\n    name: chk_age\n    expr: |\n      age > 0 AND age < 120\n";

        assert_check_expr_tokens(
            src,
            "age > 0 AND age < 120",
            &[
                ("age", TokenIdx::Property),
                (">", TokenIdx::Keyword),
                ("0", TokenIdx::Number),
                ("AND", TokenIdx::Keyword),
                ("age", TokenIdx::Property),
                ("<", TokenIdx::Keyword),
                ("120", TokenIdx::Number),
            ],
        );
    }

    #[test]
    fn bs_s2_folded_block_scalar_emits_check_tokens() {
        let src = "name: users\ncolumns:\n  - {name: id, type: integer, nullable: false, primary_key: true}\n  - {name: age, type: integer, nullable: false}\nconstraints:\n  - type: check\n    name: chk_age\n    expr: >\n      age > 0\n";

        assert_check_expr_tokens(
            src,
            "age > 0",
            &[
                ("age", TokenIdx::Property),
                (">", TokenIdx::Keyword),
                ("0", TokenIdx::Number),
            ],
        );
    }

    #[test]
    fn bs_s3_block_scalar_with_reversed_between_still_tokenizes() {
        let src = "name: users\ncolumns:\n  - {name: id, type: integer, nullable: false, primary_key: true}\n  - {name: age, type: integer, nullable: false}\nconstraints:\n  - type: check\n    name: chk_age\n    expr: |\n      age BETWEEN 100 AND 0\n";

        assert_check_expr_tokens(
            src,
            "age BETWEEN 100 AND 0",
            &[
                ("age", TokenIdx::Property),
                ("BETWEEN", TokenIdx::Keyword),
                ("100", TokenIdx::Number),
                ("AND", TokenIdx::Keyword),
                ("0", TokenIdx::Number),
            ],
        );
    }

    #[test]
    fn bs_s4_quoted_scalar_check_tokens_unchanged() {
        let src = "name: users\ncolumns:\n  - {name: id, type: integer, nullable: false, primary_key: true}\n  - {name: age, type: integer, nullable: false}\nconstraints:\n  - type: check\n    name: chk_age_range\n    expr: \"age > 0\"\n";

        assert_check_expr_tokens(
            src,
            "age > 0",
            &[
                ("age", TokenIdx::Property),
                (">", TokenIdx::Keyword),
                ("0", TokenIdx::Number),
            ],
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
        first_node(tree.root_node(), "block_mapping_pair")
            .or_else(|| first_node(tree.root_node(), "flow_pair"))
            .unwrap_or_else(|| panic!("YAML pair missing in {src}"))
    }

    #[test]
    fn classify_pair_early_returns_for_missing_key_value_and_bad_key_text() {
        let pool = ParserPool::new();
        let root_src = "name: u\n";
        let tree = pool.parse(root_src, DocumentFormat::Yaml).unwrap();
        let mut out = Vec::new();
        classify_pair(
            tree.root_node(),
            root_src.as_bytes(),
            Ctx::default(),
            &mut out,
        );

        let missing_value = "name:\n";
        let tree = pool.parse(missing_value, DocumentFormat::Yaml).unwrap();
        classify_pair(
            first_pair(&tree, missing_value),
            missing_value.as_bytes(),
            Ctx::default(),
            &mut out,
        );

        let valid = "name: u\n";
        let tree = pool.parse(valid, DocumentFormat::Yaml).unwrap();
        let pair = first_pair(&tree, valid);
        let mut bad = valid.as_bytes().to_vec();
        bad[0] = 0xff;
        classify_pair(pair, &bad, Ctx::default(), &mut out);

        assert!(
            out.iter()
                .all(|token| token.token_type != TokenIdx::Class as u32)
        );
    }

    #[test]
    fn yaml_integer_enum_member_name_and_malformed_type_value_paths() {
        let src = "name: u\ncolumns:\n  - name: priority\n    type:\n      kind: enum\n      name: priority_level\n      values:\n        - name: low\n          value: 0\n";
        let tokens = classify_yaml(src);
        let low_start = src.find("name: low").unwrap() + "name: ".len();
        let tok = tokens
            .iter()
            .find(|token| token.byte_range.start == low_start)
            .expect("integer enum name token");
        assert_eq!(tok.token_type, TokenIdx::EnumMember as u32);

        let malformed = "name: u\ncolumns:\n  - name: x\n    type: [1, 2]\n";
        let _ = classify_yaml(malformed);
    }

    #[test]
    fn classify_type_value_walks_non_scalar_non_mapping_values() {
        let pool = ParserPool::new();
        let src = "name: u\ncolumns:\n  - name: x\n    type:\n      - integer\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let type_pair = first_node(tree.root_node(), "block_mapping_pair")
            .and_then(|_| find_type_pair(tree.root_node(), src.as_bytes()))
            .expect("type pair");
        let value = type_pair.named_child(1).expect("type value");
        let mut out = Vec::new();

        classify_type_value(value, src.as_bytes(), Ctx::default(), &mut out);

        assert!(
            out.is_empty(),
            "sequence-valued type should be walked without type tokens: {out:?}"
        );
    }

    fn find_type_pair<'tree>(
        node: tree_sitter::Node<'tree>,
        source: &[u8],
    ) -> Option<tree_sitter::Node<'tree>> {
        if matches!(node.kind(), "block_mapping_pair" | "flow_pair")
            && node
                .named_child(0)
                .and_then(|key| source.get(key.byte_range()))
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .map(|text| text.trim().trim_matches('"').trim_matches('\''))
                == Some("type")
        {
            return Some(node);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_type_pair(child, source) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn check_expr_emitter_returns_for_non_scalar_empty_and_bad_sources() {
        let pool = ParserPool::new();
        let src = "name: u\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let mut out = Vec::new();
        emit_yaml_check_expr_tokens(tree.root_node(), src.as_bytes(), &mut out);

        let empty = "expr: \"\"\n";
        let tree = pool.parse(empty, DocumentFormat::Yaml).unwrap();
        let value = first_pair(&tree, empty).named_child(1).unwrap();
        emit_yaml_check_expr_tokens(value, empty.as_bytes(), &mut out);
        emit_yaml_check_expr_tokens(value, b"", &mut out);

        let valid = "expr: \"age\"\n";
        let tree = pool.parse(valid, DocumentFormat::Yaml).unwrap();
        let value = first_pair(&tree, valid).named_child(1).unwrap();
        emit_yaml_check_expr_tokens(value, b"", &mut out);
        let mut bad = valid.as_bytes().to_vec();
        let idx = valid.find("age").unwrap();
        bad[idx] = 0xff;
        emit_yaml_check_expr_tokens(value, &bad, &mut out);

        assert!(out.is_empty());
    }

    #[test]
    fn push_scalar_and_unwrap_yaml_handle_empty_scalars_and_wrappers() {
        let pool = ParserPool::new();
        let src = "name: \"\"\n";
        let tree = pool.parse(src, DocumentFormat::Yaml).unwrap();
        let value = unwrap_yaml(first_pair(&tree, src).named_child(1).unwrap());
        let mut out = Vec::new();
        push_scalar(value, TokenIdx::Class, 0, &mut out);
        assert!(out.is_empty());

        let empty = "name:\n";
        let tree = pool.parse(empty, DocumentFormat::Yaml).unwrap();
        if let Some(wrapper) = first_pair(&tree, empty).named_child(1) {
            let _ = unwrap_yaml(wrapper);
        }
    }
}
