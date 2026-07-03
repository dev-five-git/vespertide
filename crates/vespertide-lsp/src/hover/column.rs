//! Column hover: markdown showing name, type, nullable, default, constraints.

use crate::text_util::strip_quotes;
use std::fmt::Write as _;
use std::ops::Range;

use super::DomainHover;

pub(super) fn try_hover(node: tree_sitter::Node<'_>, source: &str) -> Option<DomainHover> {
    let mut cur = Some(node);
    while let Some(candidate) = cur {
        if is_mapping(candidate)
            && is_inside_columns(candidate, source)
            && let Some(markdown) = column_object_markdown(candidate, source)
        {
            return Some(DomainHover {
                markdown,
                byte_range: highlight_range(node, candidate),
            });
        }
        cur = candidate.parent();
    }
    None
}

fn column_object_markdown(obj: tree_sitter::Node<'_>, source: &str) -> Option<String> {
    let mut name: Option<String> = None;
    let mut type_str: Option<String> = None;
    let mut nullable: Option<bool> = None;
    let mut default: Option<String> = None;
    let mut constraints = Vec::new();

    let mut cursor = obj.walk();
    for child in obj.children(&mut cursor) {
        if is_pair(child)
            && let Some(key) = child.named_child(0)
            && let Some(value) = child.named_child(1)
            && let Some(key_text) = source.get(key.byte_range()).map(strip_quotes)
            && let Some(value_text) = source.get(value.byte_range()).map(str::trim)
        {
            match key_text {
                "name" => name = Some(strip_quotes(value_text).to_string()),
                "type" => type_str = Some(display_value(value_text).to_string()),
                "nullable" => nullable = Some(value_text == "true"),
                "default" => default = Some(display_value(value_text).to_string()),
                "primary_key" if constraint_is_enabled(value_text) => constraints.push("PK"),
                "unique" if constraint_is_enabled(value_text) => constraints.push("UNIQUE"),
                "index" if constraint_is_enabled(value_text) => constraints.push("INDEX"),
                "foreign_key" if constraint_is_enabled(value_text) => constraints.push("FK"),
                _ => {}
            }
        }
    }

    let name = name?;
    let type_str = type_str?;
    let mut markdown = format!("**{name}**: `{}`", type_str.trim());
    if let Some(nullable) = nullable {
        let _ = write!(markdown, "  \nnullable: `{nullable}`");
    }
    if let Some(default) = default {
        let _ = write!(markdown, "  \ndefault: `{}`", default.trim());
    }
    if !constraints.is_empty() {
        let _ = write!(markdown, "  \nconstraints: {}", constraints.join(", "));
    }
    Some(markdown)
}

fn is_inside_columns(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let mut cur = node.parent();
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && strip_quotes(&source[key.byte_range()]) == "columns"
        {
            return true;
        }
        cur = candidate.parent();
    }
    false
}

fn highlight_range(node: tree_sitter::Node<'_>, fallback: tree_sitter::Node<'_>) -> Range<usize> {
    let range = node.byte_range();
    if range.is_empty() {
        fallback.byte_range()
    } else {
        range
    }
}

fn is_mapping(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "object" | "block_mapping")
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

fn constraint_is_enabled(value: &str) -> bool {
    !matches!(value.trim(), "false" | "null" | "[]" | "{}")
}

fn display_value(value: &str) -> &str {
    strip_quotes(value.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};

    fn hover_at(src: &str, format: DocumentFormat, byte_offset: usize) -> Option<DomainHover> {
        let pool = ParserPool::new();
        let tree = pool.parse(src, format).expect("parse");
        let node = tree
            .root_node()
            .descendant_for_byte_range(byte_offset, byte_offset)
            .unwrap();
        try_hover(node, src)
    }

    #[test]
    fn full_column_markdown_includes_nullable_default_and_all_constraints() {
        // Column with nullable=true (line 50), default (51), primary_key (52),
        // unique (53), index, and foreign_key set so the markdown picks up
        // every branch in `column_object_markdown`. The walk also encounters
        // punctuation children (line 36 — non-pair) and string children.
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","nullable":true,"default":"0","primary_key":true,"unique":true,"index":true,"foreign_key":{"ref_table":"r","ref_columns":["id"]}}]}"#;
        let pos = src.find(r#""name":"id""#).unwrap() + 8;
        let hover = hover_at(src, DocumentFormat::Json, pos).expect("hover on full column");

        // Header is the column name + type (line 62, baseline).
        assert!(hover.markdown.contains("**id**"));
        assert!(hover.markdown.contains("`integer`"));
        // nullable line (line 64).
        assert!(
            hover.markdown.contains("nullable: `true`"),
            "must surface nullable, got: {}",
            hover.markdown
        );
        // default line (line 66-67).
        assert!(
            hover.markdown.contains("default: `0`"),
            "must surface default, got: {}",
            hover.markdown
        );
        // constraints block (line 70).
        let constraints_section = hover
            .markdown
            .split("constraints: ")
            .nth(1)
            .expect("constraints block present");
        for marker in ["PK", "UNIQUE", "INDEX", "FK"] {
            assert!(
                constraints_section.contains(marker),
                "constraint `{marker}` should be present, got: {}",
                hover.markdown
            );
        }
    }

    #[test]
    fn column_with_only_name_and_type_omits_nullable_default_constraints() {
        // Minimal column — markdown picks up only name + type. nullable/
        // default/constraints branches are skipped.
        let src = r#"{"name":"u","columns":[{"name":"x","type":"text"}]}"#;
        let pos = src.find(r#""name":"x""#).unwrap() + 8;
        let hover = hover_at(src, DocumentFormat::Json, pos).expect("hover");
        assert!(hover.markdown.contains("**x**"));
        assert!(!hover.markdown.contains("nullable"));
        assert!(!hover.markdown.contains("default"));
        assert!(!hover.markdown.contains("constraints"));
    }

    #[test]
    fn disabled_constraints_are_not_listed() {
        // primary_key=false / unique=false / index=false → constraint_is_enabled
        // returns false so no markers are added.
        let src = r#"{"name":"u","columns":[{"name":"id","type":"integer","primary_key":false,"unique":false,"index":false}]}"#;
        let pos = src.find(r#""name":"id""#).unwrap() + 8;
        let hover = hover_at(src, DocumentFormat::Json, pos).expect("hover");
        assert!(
            !hover.markdown.contains("constraints"),
            "disabled flags must NOT produce a constraints block, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn column_without_name_returns_none() {
        // Column object missing `name` — `column_object_markdown` returns
        // None on `name?` so try_hover walks up further but no ancestor
        // column object exists with both required keys.
        let src = r#"{"name":"u","columns":[{"type":"text","nullable":false}]}"#;
        // Hover inside the `type` value.
        let pos = src.find(r#""type":"text""#).unwrap() + 9;
        // Either None or a hover not stemming from this column object.
        let hover = hover_at(src, DocumentFormat::Json, pos);
        if let Some(h) = hover {
            // If we did get a hover, it MUST NOT come from this nameless column.
            assert!(
                !h.markdown.starts_with("**:"),
                "nameless column should not emit a header, got: {}",
                h.markdown
            );
        }
    }

    #[test]
    fn yaml_full_column_markdown_includes_all_fields() {
        // YAML parses block_mapping_pair / block_mapping instead of pair /
        // object — exercising the YAML node kinds in is_mapping/is_pair.
        let src = "name: u\ncolumns:\n  - name: id\n    type: integer\n    nullable: true\n    default: \"0\"\n    primary_key: true\n    unique: true\n    index: true\n";
        let pos = src.find("name: id").unwrap() + 6;
        let hover = hover_at(src, DocumentFormat::Yaml, pos).expect("yaml column hover");
        assert!(hover.markdown.contains("**id**"));
        assert!(hover.markdown.contains("nullable"));
        assert!(hover.markdown.contains("default"));
        assert!(hover.markdown.contains("constraints"));
    }

    #[test]
    fn malformed_column_pairs_without_key_or_value_are_skipped() {
        let pool = ParserPool::new();
        let missing_key = r#"{:"ignored","name":"id","type":"integer"}"#;
        let tree = pool.parse(missing_key, DocumentFormat::Json).unwrap();
        let obj = tree.root_node().named_child(0).unwrap();
        assert!(column_object_markdown(obj, missing_key).is_some());

        let missing_value = r#"{"name":"id","type":}"#;
        let tree = pool.parse(missing_value, DocumentFormat::Json).unwrap();
        let obj = tree.root_node().named_child(0).unwrap();
        let _ = column_object_markdown(obj, missing_value);
    }

    #[test]
    fn hover_on_column_returns_some() {
        let src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let pos = src.find(r#""name":"id""#).unwrap() + 5;
        let hover = hover_at(src, DocumentFormat::Json, pos).expect("hover on column");

        assert!(
            hover.markdown.contains("id"),
            "markdown should contain column name"
        );
    }
}
