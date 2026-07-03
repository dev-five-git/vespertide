//! CHECK-expression hover: hovering inside a `constraints[*].expr`
//! string shows a markdown popup describing the parsed structure.
//!
//! Dispatched **first** in `hover::mod.rs` so a bare column identifier
//! that happens to sit inside a CHECK expression is interpreted as
//! check-expr context, not as a column-declaration object hover.

use crate::text_util::strip_quotes;
use std::fmt::Write as _;
use vespertide_planner::{CheckExprAst, CheckExprLiteral, CheckExprOp, parse_check_expr};

use super::DomainHover;
use crate::check_expr_range::expr_inner_range;

pub(super) fn try_hover(
    node: tree_sitter::Node<'_>,
    source: &str,
    byte_offset: usize,
) -> Option<DomainHover> {
    let pair = expr_pair_ancestor(node, source)?;
    if !is_inside_constraints(pair, source) {
        return None;
    }

    let value = pair.named_child(1)?;
    let inner = expr_inner_range(value)?;
    // The cursor must actually fall inside the expr value (not the key
    // or whitespace before `:`); otherwise let other handlers run.
    if !inner.contains(&byte_offset) && byte_offset != inner.end {
        return None;
    }

    let expr_text = source.get(inner.clone())?;
    let ast = parse_check_expr(expr_text);
    Some(DomainHover {
        markdown: render_markdown(&ast, expr_text),
        byte_range: inner,
    })
}

fn render_markdown(ast: &CheckExprAst, expr_text: &str) -> String {
    let mut md = String::new();
    if matches!(ast, CheckExprAst::Unparseable) {
        let _ = write!(
            md,
            "**CHECK expression** _(could not parse structure)_\n\n`{}`",
            expr_text.trim()
        );
    } else {
        let header = header_for(ast);
        let _ = write!(md, "**{header}**\n\n`{}`", expr_text.trim());
        let bullets = bullets_for(ast);
        if !bullets.is_empty() {
            md.push_str("\n\n");
            for line in bullets {
                let _ = writeln!(md, "- {line}");
            }
        }
    }
    md
}

fn header_for(ast: &CheckExprAst) -> String {
    match ast {
        CheckExprAst::And(parts) => format!("Logical AND of {} conditions", parts.len()),
        CheckExprAst::Or(parts) => format!("Logical OR of {} conditions", parts.len()),
        CheckExprAst::Not(_) => "Logical NOT (negated condition)".to_string(),
        CheckExprAst::Compare { .. } => "Comparison predicate".to_string(),
        CheckExprAst::In { negated, .. } => {
            if *negated {
                "NOT IN list predicate".to_string()
            } else {
                "IN list predicate".to_string()
            }
        }
        CheckExprAst::Between { negated, .. } => {
            if *negated {
                "NOT BETWEEN range predicate".to_string()
            } else {
                "BETWEEN range predicate".to_string()
            }
        }
        CheckExprAst::IsNull { negated, .. } => {
            if *negated {
                "IS NOT NULL predicate".to_string()
            } else {
                "IS NULL predicate".to_string()
            }
        }
        CheckExprAst::Unparseable => "CHECK expression".to_string(),
    }
}

fn bullets_for(ast: &CheckExprAst) -> Vec<String> {
    match ast {
        CheckExprAst::And(parts) | CheckExprAst::Or(parts) => {
            parts.iter().map(render_inline).collect()
        }
        CheckExprAst::Not(inner) => vec![render_inline(inner)],
        CheckExprAst::Compare { column, op, value } => vec![format!(
            "column `{column}` {} {}",
            render_op(*op),
            render_literal(value)
        )],
        CheckExprAst::In {
            column,
            values,
            negated,
        } => {
            let mut lines = vec![format!(
                "column `{column}` {}IN list of {} value{}",
                if *negated { "NOT " } else { "" },
                values.len(),
                if values.len() == 1 { "" } else { "s" }
            )];
            for v in values {
                lines.push(format!("value {}", render_literal(v)));
            }
            lines
        }
        CheckExprAst::Between {
            column,
            low,
            high,
            negated,
        } => vec![format!(
            "column `{column}` {}BETWEEN {} AND {}",
            if *negated { "NOT " } else { "" },
            render_literal(low),
            render_literal(high)
        )],
        CheckExprAst::IsNull { column, negated } => vec![format!(
            "column `{column}` IS {}NULL",
            if *negated { "NOT " } else { "" }
        )],
        CheckExprAst::Unparseable => Vec::new(),
    }
}

fn render_inline(ast: &CheckExprAst) -> String {
    match ast {
        CheckExprAst::Compare { column, op, value } => format!(
            "condition `{column} {} {}`",
            render_op(*op),
            render_literal(value)
        ),
        CheckExprAst::In {
            column,
            values,
            negated,
        } => format!(
            "condition `{column} {}IN ({})`",
            if *negated { "NOT " } else { "" },
            values
                .iter()
                .map(render_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        CheckExprAst::Between {
            column,
            low,
            high,
            negated,
        } => format!(
            "condition `{column} {}BETWEEN {} AND {}`",
            if *negated { "NOT " } else { "" },
            render_literal(low),
            render_literal(high)
        ),
        CheckExprAst::IsNull { column, negated } => format!(
            "condition `{column} IS {}NULL`",
            if *negated { "NOT " } else { "" }
        ),
        CheckExprAst::And(parts) => {
            format!("nested AND of {} conditions", parts.len())
        }
        CheckExprAst::Or(parts) => {
            format!("nested OR of {} conditions", parts.len())
        }
        CheckExprAst::Not(_) => "nested NOT condition".to_string(),
        CheckExprAst::Unparseable => "unparseable sub-expression".to_string(),
    }
}

fn render_op(op: CheckExprOp) -> &'static str {
    match op {
        CheckExprOp::Eq => "=",
        CheckExprOp::Ne => "<>",
        CheckExprOp::Lt => "<",
        CheckExprOp::Le => "<=",
        CheckExprOp::Gt => ">",
        CheckExprOp::Ge => ">=",
    }
}

fn render_literal(lit: &CheckExprLiteral) -> String {
    match lit {
        CheckExprLiteral::Integer(i) => i.to_string(),
        CheckExprLiteral::Float(f) => f.to_string(),
        CheckExprLiteral::String(s) => s.clone(),
        CheckExprLiteral::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        CheckExprLiteral::Null => "NULL".to_string(),
    }
}

/// Walk up from `node` looking for a pair `"expr": <scalar>` (JSON
/// `pair` or YAML `block_mapping_pair`).
fn expr_pair_ancestor<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cur = Some(node);
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && source
                .get(key.byte_range())
                .is_some_and(|text| strip_quotes(text) == "expr")
        {
            return Some(candidate);
        }
        cur = candidate.parent();
    }
    None
}

/// True when any ancestor pair has key `"constraints"`. Mirrors
/// `column::is_inside_columns` from the column-hover handler.
fn is_inside_constraints(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let mut cur = node.parent();
    while let Some(candidate) = cur {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && source
                .get(key.byte_range())
                .is_some_and(|text| strip_quotes(text) == "constraints")
        {
            return true;
        }
        cur = candidate.parent();
    }
    false
}

fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{DocumentFormat, ParserPool};
    use crate::store::DocumentStore;
    use crate::test_support::uri;
    use crate::workspace_index::WorkspaceIndex;

    fn hover_at(src: &str, format: DocumentFormat, byte_offset: usize) -> Option<DomainHover> {
        let pool = ParserPool::new();
        let tree = pool.parse(src, format).expect("source should parse");
        let node = tree
            .root_node()
            .descendant_for_byte_range(byte_offset, byte_offset)
            .expect("cursor should resolve to a node");
        try_hover(node, src, byte_offset)
    }

    #[test]
    fn hover_json_and_structure() {
        let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0 AND age < 150"}]}"#;
        let offset = src.find("AND").expect("AND present") + 1;

        let hover = hover_at(src, DocumentFormat::Json, offset)
            .expect("hover inside JSON CHECK expr should return Some");

        assert!(
            hover.markdown.contains("AND") && hover.markdown.contains("age < 150"),
            "markdown should describe AND structure, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_yaml_block_scalar() {
        let src = r"name: users
columns:
  - name: age
    type: integer
    nullable: false
constraints:
  - type: check
    name: chk_age
    expr: |
      age > 0 AND age < 150
";
        let offset = src.find("age > 0").expect("expr present") + 2;

        let hover = hover_at(src, DocumentFormat::Yaml, offset)
            .expect("hover inside YAML block CHECK expr should return Some");

        assert!(
            hover.markdown.contains("age > 0") && hover.markdown.contains("AND"),
            "markdown should reflect YAML block expr, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_cursor_at_expr_end_boundary() {
        let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;
        let expr_start = src.find("age > 0").expect("expr present");
        let expr_end = expr_start + "age > 0".len();

        assert!(
            hover_at(src, DocumentFormat::Json, expr_end - 1).is_some(),
            "hover should work on the last byte inside the CHECK expr"
        );
        assert!(
            hover_at(src, DocumentFormat::Json, expr_end).is_some(),
            "hover should work at the expr inner.end exclusive boundary"
        );
        assert_eq!(
            hover_at(src, DocumentFormat::Json, expr_end + 1),
            None,
            "hover should not work one byte past the expr boundary"
        );
    }

    #[test]
    fn hover_outside_constraints_returns_none() {
        let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;
        let offset = src.find("integer").expect("column type present") + 2;

        assert_eq!(hover_at(src, DocumentFormat::Json, offset), None);
    }

    // ============================================================
    // Branch coverage for `render_markdown` / `header_for` /
    // `bullets_for` / `render_inline` — one CHECK shape per AST
    // variant. Every test asserts on a marker that proves the
    // corresponding `match` arm executed.
    // ============================================================

    fn make_src(expr: &str) -> String {
        // Embed the CHECK expression in a minimal valid model. The
        // column list referenced by the expression is irrelevant for
        // rendering — the AST is parsed standalone.
        format!(
            r#"{{"name":"t","columns":[{{"name":"x","type":"integer","nullable":false}},{{"name":"y","type":"integer","nullable":false}}],"constraints":[{{"type":"check","name":"c","expr":"{expr}"}}]}}"#
        )
    }

    fn hover_for_expr(expr: &str) -> DomainHover {
        let src = make_src(expr);
        // Position the cursor on the FIRST char of the expr value (which
        // is always a Column or NOT/IS keyword token).
        let offset = src.find(expr).expect("expr embedded") + 1;
        hover_at(&src, DocumentFormat::Json, offset).unwrap_or_else(|| {
            // If the cursor lands on a non-token byte some expressions
            // need a deeper offset; retry mid-expression.
            let off2 = src.find(expr).unwrap() + expr.len() / 2;
            hover_at(&src, DocumentFormat::Json, off2)
                .expect("hover must succeed somewhere inside expr")
        })
    }

    #[test]
    fn hover_or_renders_logical_or_header_and_two_bullets() {
        let hover = hover_for_expr("x > 0 OR y < 10");
        assert!(
            hover.markdown.contains("Logical OR of 2"),
            "{}",
            hover.markdown
        );
        assert!(hover.markdown.contains("x > 0"));
        assert!(hover.markdown.contains("y < 10"));
    }

    #[test]
    fn hover_not_renders_logical_not_header() {
        let hover = hover_for_expr("NOT x > 0");
        assert!(
            hover.markdown.contains("Logical NOT"),
            "must mention NOT, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_compare_renders_comparison_predicate_header() {
        let hover = hover_for_expr("x = 5");
        assert!(
            hover.markdown.contains("Comparison predicate"),
            "got: {}",
            hover.markdown
        );
        assert!(hover.markdown.contains("column `x`"));
        assert!(hover.markdown.contains('='));
    }

    #[test]
    fn hover_compare_covers_every_operator_variant() {
        // Each op flushes a distinct `render_op` arm.
        for (op, sym) in [
            ("=", "="),
            ("!=", "<>"),
            ("<>", "<>"),
            ("<", "<"),
            ("<=", "<="),
            (">", ">"),
            (">=", ">="),
        ] {
            let hover = hover_for_expr(&format!("x {op} 1"));
            assert!(
                hover.markdown.contains(sym),
                "render_op for `{op}` should produce `{sym}` in markdown, got: {}",
                hover.markdown
            );
        }
    }

    #[test]
    fn hover_compare_covers_every_literal_variant() {
        // Integer
        let h = hover_for_expr("x = 7");
        assert!(h.markdown.contains('7'));
        // Float
        let h = hover_for_expr("x = 1.5");
        assert!(h.markdown.contains("1.5"));
        // String literal
        let h = hover_for_expr("x = 'foo'");
        assert!(h.markdown.contains("'foo'"));
        // Bool literals — TRUE and FALSE branches.
        let h = hover_for_expr("x = true");
        assert!(h.markdown.contains("TRUE"));
        let h = hover_for_expr("x = false");
        assert!(h.markdown.contains("FALSE"));
        // Null
        let h = hover_for_expr("x = null");
        assert!(h.markdown.contains("NULL"));
    }

    #[test]
    fn hover_in_list_predicate_renders_header_and_values() {
        let hover = hover_for_expr("x IN (1, 2)");
        assert!(hover.markdown.contains("IN list predicate"));
        // bullets list each value (line 116-118).
        assert!(hover.markdown.contains("value 1"));
        assert!(hover.markdown.contains("value 2"));
    }

    #[test]
    fn hover_in_list_single_value_uses_singular_form() {
        let hover = hover_for_expr("x IN (1)");
        assert!(
            hover.markdown.contains("list of 1 value"),
            "single-value IN should pluralise correctly, got: {}",
            hover.markdown
        );
    }

    #[test]
    fn hover_not_in_list_predicate_negated_header() {
        let hover = hover_for_expr("x NOT IN (1, 2)");
        assert!(
            hover.markdown.contains("NOT IN list predicate"),
            "NOT IN header must surface, got: {}",
            hover.markdown
        );
        assert!(hover.markdown.contains("NOT IN list"));
    }

    #[test]
    fn hover_between_predicate_renders_range() {
        let hover = hover_for_expr("x BETWEEN 1 AND 10");
        assert!(hover.markdown.contains("BETWEEN range predicate"));
        assert!(hover.markdown.contains("BETWEEN 1 AND 10"));
    }

    #[test]
    fn hover_not_between_predicate_negated_header() {
        let hover = hover_for_expr("x NOT BETWEEN 1 AND 10");
        assert!(
            hover.markdown.contains("NOT BETWEEN range predicate"),
            "NOT BETWEEN header must surface, got: {}",
            hover.markdown
        );
        assert!(hover.markdown.contains("NOT BETWEEN 1 AND 10"));
    }

    #[test]
    fn hover_is_null_predicate_header() {
        let hover = hover_for_expr("x IS NULL");
        assert!(hover.markdown.contains("IS NULL predicate"));
        assert!(hover.markdown.contains("IS NULL"));
        assert!(!hover.markdown.contains("IS NOT NULL"));
    }

    #[test]
    fn hover_is_not_null_predicate_header() {
        let hover = hover_for_expr("x IS NOT NULL");
        assert!(hover.markdown.contains("IS NOT NULL predicate"));
        assert!(hover.markdown.contains("IS NOT NULL"));
    }

    #[test]
    fn hover_unparseable_expression_uses_could_not_parse_branch() {
        // `LOWER(x) = 1` is outside the dialect-neutral subset.
        let src = make_src("LOWER(x) = 1");
        let offset = src.find("LOWER").expect("expr present") + 2;
        let hover = hover_at(&src, DocumentFormat::Json, offset).expect("unparseable hover");
        assert!(
            hover.markdown.contains("could not parse"),
            "Unparseable header must fire, got: {}",
            hover.markdown
        );
        // bullets_for Unparseable → empty; render_markdown does not append
        // the `-` bullets section, so the markdown ends with the backticked
        // expression.
        assert!(!hover.markdown.contains("- condition"));
    }

    #[test]
    fn hover_render_inline_covers_nested_and_or_not() {
        // The OUTER AST is OR-of-3 so each bullet flows through
        // `render_inline`, exercising the nested-AND, nested-OR and
        // nested-NOT arms.
        let hover = hover_for_expr("(x > 0 AND y > 0) OR (x > 0 OR y > 0) OR NOT x > 0");
        // Header is Logical OR of 3 conditions.
        assert!(hover.markdown.contains("Logical OR of 3"));
        assert!(hover.markdown.contains("nested AND"));
        assert!(hover.markdown.contains("nested OR"));
        assert!(hover.markdown.contains("nested NOT"));
    }

    #[test]
    fn hover_render_inline_covers_in_between_isnull_subexpressions() {
        // OR over IN / BETWEEN / IS NULL bullets so render_inline hits
        // every concrete-predicate arm.
        let hover = hover_for_expr("x IN (1, 2) OR x BETWEEN 1 AND 10 OR x IS NULL");
        // Each nested predicate gets formatted as `condition `...``
        assert!(
            hover.markdown.contains("IN ("),
            "IN subexpr, got: {}",
            hover.markdown
        );
        assert!(hover.markdown.contains("BETWEEN 1 AND 10"));
        assert!(hover.markdown.contains("IS NULL"));
    }

    #[test]
    fn hover_render_inline_negated_in_between_isnotnull() {
        // OR over NOT IN / NOT BETWEEN / IS NOT NULL flushes the
        // `negated` branches of render_inline (NOT prefix).
        let hover = hover_for_expr("x NOT IN (1) OR x NOT BETWEEN 1 AND 5 OR x IS NOT NULL");
        assert!(hover.markdown.contains("NOT IN"));
        assert!(hover.markdown.contains("NOT BETWEEN"));
        assert!(hover.markdown.contains("IS NOT NULL"));
    }

    #[test]
    fn hover_cursor_before_expr_returns_none() {
        // Cursor sits on the `:` between the `expr` key and value — outside
        // the inner expression range, so try_hover returns None.
        let src = r#"{"name":"t","columns":[{"name":"x","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"x > 0"}]}"#;
        let colon = src.find(r#""expr":"#).expect("expr key present") + 6;
        // Just before the opening quote.
        assert_eq!(hover_at(src, DocumentFormat::Json, colon), None);
    }

    #[test]
    fn hover_expr_outside_constraints_returns_none() {
        let src = r#"{"name":"t","expr":"x > 0","columns":[{"name":"x","type":"integer"}]}"#;
        let offset = src.find("x > 0").unwrap() + 1;
        assert_eq!(hover_at(src, DocumentFormat::Json, offset), None);
    }

    #[test]
    fn direct_unparseable_render_helpers_cover_defensive_arms() {
        let ast = CheckExprAst::Unparseable;
        assert_eq!(header_for(&ast), "CHECK expression");
        assert!(bullets_for(&ast).is_empty());
        assert_eq!(render_inline(&ast), "unparseable sub-expression");
    }

    #[test]
    fn compute_hover_inside_and_describes_structure() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"users","columns":[{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0 AND age < 150"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let needle = "age > 0 AND";
        let pos = src.find(needle).expect("needle present") + needle.len() - 2;

        let hover =
            super::super::compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos)
                .expect("hover inside parseable CHECK expression");

        assert!(
            hover.markdown.contains("AND"),
            "markdown should describe the AND structure, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.to_lowercase().contains("condition")
                || hover.markdown.to_lowercase().contains("predicate"),
            "markdown should refer to conditions/predicates, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.contains("age > 0"),
            "markdown should mention the first sub-expression `age > 0`, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.contains("age < 150"),
            "markdown should mention the second sub-expression `age < 150`, got: {}",
            hover.markdown
        );
        let expr_inner_start = src.find(r#""expr":""#).unwrap() + r#""expr":""#.len();
        let expr_inner_end = src[expr_inner_start..].find('"').unwrap() + expr_inner_start;
        assert!(
            hover.byte_range.start >= expr_inner_start
                && hover.byte_range.end <= expr_inner_end
                && hover.byte_range.start < hover.byte_range.end,
            "byte_range must lie inside the expr value [{expr_inner_start}..{expr_inner_end}), got {:?}",
            hover.byte_range
        );
    }

    #[test]
    fn compute_hover_unparseable_is_graceful() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let src = r#"{"name":"t","columns":[{"name":"x","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"c","expr":"LOWER(x) = 1"}]}"#;
        let tree = pool.parse(src, DocumentFormat::Json);
        let pos = src.find("LOWER(").expect("needle present") + 2;

        let hover =
            super::super::compute(src, DocumentFormat::Json, tree.as_ref(), &idx, &docs, pos);

        if let Some(h) = hover {
            let lower = h.markdown.to_lowercase();
            assert!(
                lower.contains("check") && (lower.contains("parse") || lower.contains("structure")),
                "Unparseable-case markdown must mention CHECK + parse/structure, got: {}",
                h.markdown
            );
        }
    }

    #[test]
    fn compute_hover_on_ref_table_still_works_after_check_dispatch() {
        let pool = ParserPool::new();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let user_uri = uri("user.json");
        let user_src = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true}]}"#;
        let user_tree = pool.parse(user_src, DocumentFormat::Json).unwrap();
        idx.upsert(&user_uri, user_src, &user_tree);
        docs.open(user_uri, "json".to_string(), 1, user_src.to_string());
        let post_src = r#"{"name":"post","columns":[{"name":"author_id","type":"integer","nullable":false,"foreign_key":{"ref_table":"user","ref_columns":["id"]}}],"constraints":[{"type":"check","name":"chk_pos","expr":"author_id > 0"}]}"#;
        let post_tree = pool.parse(post_src, DocumentFormat::Json);
        let pos = post_src.find(r#""ref_table":"user""#).unwrap() + 14;

        let hover = super::super::compute(
            post_src,
            DocumentFormat::Json,
            post_tree.as_ref(),
            &idx,
            &docs,
            pos,
        )
        .expect("hover on ref_table must still resolve");

        assert!(
            hover.markdown.contains("Target table"),
            "FK hover should still produce the target-table preview, got: {}",
            hover.markdown
        );
        assert!(
            hover.markdown.contains("user"),
            "FK hover should still mention the target table name, got: {}",
            hover.markdown
        );
    }
}
