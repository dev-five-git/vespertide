//! Kept manual-QA integration coverage for CHECK diagnostics and semantic tokens.

use std::ops::Range;

use vespertide_lsp::diagnostics::Severity;
use vespertide_lsp::semantic_tokens::RawToken;
use vespertide_lsp::semantic_tokens::classify;
use vespertide_lsp::semantic_tokens::legend::TokenIdx;
use vespertide_lsp::{
    DocumentFormat, DocumentStore, DomainDiagnostic, ParserPool, WorkspaceIndex,
    compute_completion, compute_diagnostics, compute_hover, compute_inlay_hints,
};

const JSON_MODEL: &str = r#"{
  "name": "users",
  "columns": [
    {"name": "id", "type": "integer", "nullable": false, "primary_key": true},
    {"name": "age", "type": "integer", "nullable": false},
    {"name": "status", "type": "text", "nullable": false}
  ],
  "constraints": [
    {"type": "check", "name": "chk_age_valid", "expr": "age > 0 AND age < 150"},
    {"type": "check", "name": "chk_age_reversed", "expr": "age BETWEEN 100 AND 0"},
    {"type": "check", "name": "chk_age_contradiction", "expr": "age > 100 AND age < 0"},
    {"type": "check", "name": "chk_status_typemismatch", "expr": "age = 'abc'"}
  ]
}"#;

const YAML_MODEL: &str = r#"name: users
columns:
  - name: id
    type: integer
    nullable: false
    primary_key: true
  - name: age
    type: integer
    nullable: false
  - name: status
    type: text
    nullable: false
constraints:
  - type: check
    name: chk_status_typemismatch
    expr: "age = 'abc'"
"#;

#[test]
fn json_check_diagnostics_and_semantic_tokens_render_user_visible_output() {
    let pool = ParserPool::new();
    let index = WorkspaceIndex::new();
    let tree = pool
        .parse(JSON_MODEL, DocumentFormat::Json)
        .expect("JSON model parses");

    let diagnostics = compute_diagnostics(JSON_MODEL, DocumentFormat::Json, Some(&tree), &index);
    let tokens = classify(JSON_MODEL, DocumentFormat::Json, Some(&tree));
    let valid_expr_range = source_range(JSON_MODEL, "age > 0 AND age < 150");
    let valid_tokens = tokens_in_range(&tokens, valid_expr_range.clone());

    println!("=== DIAGNOSTICS (JSON model) ===");
    print_diagnostics(JSON_MODEL, &diagnostics);
    println!("=== SEMANTIC TOKENS (chk_age_valid expr) ===");
    print_tokens(JSON_MODEL, &valid_tokens);

    assert_diagnostic_count(
        JSON_MODEL,
        &diagnostics,
        Severity::Error,
        "check-between-reversed",
        "chk_age_reversed",
        1,
    );
    assert_diagnostic_count(
        JSON_MODEL,
        &diagnostics,
        Severity::Error,
        "check-self-contradiction",
        "chk_age_contradiction",
        1,
    );
    assert_diagnostic_count(
        JSON_MODEL,
        &diagnostics,
        Severity::Warning,
        "check-type-mismatch",
        "age = 'abc'",
        1,
    );
    assert!(
        diagnostics
            .iter()
            .filter(|diag| slice(JSON_MODEL, diag.byte_range.clone()).contains("age > 0"))
            .all(|diag| diag.severity != Severity::Error && diag.code != "check-type-mismatch"),
        "valid CHECK expression must not receive CHECK diagnostics: {diagnostics:?}"
    );

    let type_mismatch = diagnostic_for_slice(&diagnostics, JSON_MODEL, "age = 'abc'")
        .expect("type mismatch diagnostic present");
    assert_eq!(type_mismatch.code, "check-type-mismatch");
    assert_eq!(type_mismatch.severity, Severity::Warning);

    assert_constraint_anchored(
        JSON_MODEL,
        &diagnostics,
        "check-between-reversed",
        "chk_age_reversed",
        "age BETWEEN 100 AND 0",
    );
    assert_constraint_anchored(
        JSON_MODEL,
        &diagnostics,
        "check-self-contradiction",
        "chk_age_contradiction",
        "age > 100 AND age < 0",
    );

    assert_token_sequence(
        JSON_MODEL,
        &valid_tokens,
        &[
            (TokenIdx::Property as u32, "age"),
            (TokenIdx::Keyword as u32, ">"),
            (TokenIdx::Number as u32, "0"),
            (TokenIdx::Keyword as u32, "AND"),
            (TokenIdx::Property as u32, "age"),
            (TokenIdx::Keyword as u32, "<"),
            (TokenIdx::Number as u32, "150"),
        ],
    );
}

#[test]
fn yaml_check_type_mismatch_and_tokens_render_user_visible_output() {
    let pool = ParserPool::new();
    let index = WorkspaceIndex::new();
    let tree = pool
        .parse(YAML_MODEL, DocumentFormat::Yaml)
        .expect("YAML model parses");

    let diagnostics = compute_diagnostics(YAML_MODEL, DocumentFormat::Yaml, Some(&tree), &index);
    let tokens = classify(YAML_MODEL, DocumentFormat::Yaml, Some(&tree));
    let expr_range = source_range(YAML_MODEL, "age = 'abc'");
    let expr_tokens = tokens_in_range(&tokens, expr_range);

    println!("=== DIAGNOSTICS (YAML model) ===");
    print_diagnostics(YAML_MODEL, &diagnostics);
    println!("=== SEMANTIC TOKENS (YAML CHECK expr) ===");
    print_tokens(YAML_MODEL, &expr_tokens);

    assert_diagnostic_count(
        YAML_MODEL,
        &diagnostics,
        Severity::Warning,
        "check-type-mismatch",
        "age = 'abc'",
        1,
    );
    let type_mismatch = diagnostic_for_slice(&diagnostics, YAML_MODEL, "age = 'abc'")
        .expect("YAML type mismatch diagnostic present");
    assert_eq!(type_mismatch.code, "check-type-mismatch");

    assert_token_sequence(
        YAML_MODEL,
        &expr_tokens,
        &[
            (TokenIdx::Property as u32, "age"),
            (TokenIdx::Keyword as u32, "="),
            (TokenIdx::String as u32, "'abc'"),
        ],
    );
}

fn assert_diagnostic_count(
    source: &str,
    diagnostics: &[DomainDiagnostic],
    severity: Severity,
    code: &str,
    expected_slice: &str,
    expected_count: usize,
) {
    let count = diagnostics
        .iter()
        .filter(|diag| {
            diag.severity == severity
                && diag.code == code
                && slice(source, diag.byte_range.clone()).contains(expected_slice)
        })
        .count();

    assert_eq!(
        count, expected_count,
        "expected {expected_count} {severity:?} {code} diagnostics for {expected_slice:?}, got {diagnostics:?}"
    );
}

fn assert_constraint_anchored(
    source: &str,
    diagnostics: &[DomainDiagnostic],
    code: &str,
    constraint_name: &str,
    expr: &str,
) {
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == code)
        .unwrap_or_else(|| panic!("expected diagnostic with code {code}"));
    let snippet = slice(source, diag.byte_range.clone());

    assert!(
        snippet.contains(constraint_name) || snippet.contains(expr),
        "{code} should be anchored on constraint {constraint_name:?} / expr {expr:?}, got {snippet:?}"
    );
    assert!(
        !snippet.contains(r#""name": "age""#),
        "{code} must not be anchored on the age column declaration, got {snippet:?}"
    );
}

fn assert_token_sequence(source: &str, actual: &[&RawToken], expected: &[(u32, &str)]) {
    let rendered = actual
        .iter()
        .map(|token| (token.token_type, slice(source, token.byte_range.clone())))
        .collect::<Vec<_>>();

    assert_eq!(rendered, expected, "unexpected CHECK token stream");
}

fn diagnostic_for_slice<'a>(
    diagnostics: &'a [DomainDiagnostic],
    source: &str,
    expected_slice: &str,
) -> Option<&'a DomainDiagnostic> {
    diagnostics
        .iter()
        .find(|diag| slice(source, diag.byte_range.clone()).contains(expected_slice))
}

fn tokens_in_range(tokens: &[RawToken], range: Range<usize>) -> Vec<&RawToken> {
    tokens
        .iter()
        .filter(|token| range.start <= token.byte_range.start && token.byte_range.end <= range.end)
        .collect()
}

fn print_diagnostics(source: &str, diagnostics: &[DomainDiagnostic]) {
    for diag in diagnostics {
        println!(
            "[{}] {} @ {:?} {:?}: {}",
            severity_name(diag.severity),
            diag.code,
            diag.byte_range,
            slice(source, diag.byte_range.clone()),
            diag.message
        );
    }
}

fn print_tokens(source: &str, tokens: &[&RawToken]) {
    for token in tokens {
        println!(
            "{} @ {:?} {:?}",
            token_type_name(token.token_type),
            token.byte_range,
            slice(source, token.byte_range.clone())
        );
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Information => "Information",
        Severity::Hint => "Hint",
    }
}

fn token_type_name(token_type: u32) -> &'static str {
    match token_type {
        value if value == TokenIdx::Class as u32 => "class",
        value if value == TokenIdx::Property as u32 => "property",
        value if value == TokenIdx::Type as u32 => "type",
        value if value == TokenIdx::EnumMember as u32 => "enumMember",
        value if value == TokenIdx::Keyword as u32 => "keyword",
        value if value == TokenIdx::Number as u32 => "number",
        value if value == TokenIdx::String as u32 => "string",
        _ => "unknown",
    }
}

#[test]
fn debug_name_helpers_cover_all_remaining_display_arms() {
    assert_eq!(severity_name(Severity::Information), "Information");
    assert_eq!(severity_name(Severity::Hint), "Hint");
    assert_eq!(token_type_name(u32::MAX), "unknown");
}

fn source_range(source: &str, needle: &str) -> Range<usize> {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("source must contain {needle:?}"));
    start..(start + needle.len())
}

fn slice(source: &str, range: Range<usize>) -> &str {
    source
        .get(range.clone())
        .unwrap_or_else(|| panic!("diagnostic/token range {range:?} must be a UTF-8 boundary"))
}

// =====================================================================
// Phase E / F / F-blockscalar / G — REAL-SURFACE QA scenarios
// Each scenario drives a public LSP entry point through tree-sitter +
// the existing harness (ParserPool / WorkspaceIndex / DocumentStore).
// Entry points exercised:
//   * semantic_tokens::classify        (BS-S1)
//   * compute_hover                    (H-S1)
//   * compute_inlay_hints              (I-S1)
//   * compute_completion               (CMP-S1, CMP-S3)
// =====================================================================

/// BS-S1: YAML block scalar `expr: |` containing
/// `age > 0 AND age < 120` must produce CHECK tokens at absolute
/// positions INSIDE the block body (property/keyword/number) when the
/// real `semantic_tokens::classify` entry point is driven.
#[test]
fn bs_s1_block_scalar_check_tokens_real_surface() {
    let pool = ParserPool::new();
    let src = concat!(
        "name: users\n",
        "columns:\n",
        "  - {name: id, type: integer, nullable: false, primary_key: true}\n",
        "  - {name: age, type: integer, nullable: false}\n",
        "constraints:\n",
        "  - type: check\n",
        "    name: chk_age_range\n",
        "    expr: |\n",
        "      age > 0 AND age < 120\n",
    );

    let tree = pool
        .parse(src, DocumentFormat::Yaml)
        .expect("YAML block-scalar model parses");
    let tokens = classify(src, DocumentFormat::Yaml, Some(&tree));
    let expr_range = source_range(src, "age > 0 AND age < 120");
    let expr_tokens = tokens_in_range(&tokens, expr_range.clone());

    println!("=== BLOCK SCALAR TOKENS (bs_s1) ===");
    println!(
        "expr inner-body byte range: {expr_range:?} -> {:?}",
        slice(src, expr_range.clone())
    );
    print_tokens(src, &expr_tokens);

    assert_token_sequence(
        src,
        &expr_tokens,
        &[
            (TokenIdx::Property as u32, "age"),
            (TokenIdx::Keyword as u32, ">"),
            (TokenIdx::Number as u32, "0"),
            (TokenIdx::Keyword as u32, "AND"),
            (TokenIdx::Property as u32, "age"),
            (TokenIdx::Keyword as u32, "<"),
            (TokenIdx::Number as u32, "120"),
        ],
    );
}

/// H-S1: JSON model with `expr: "age > 0 AND age < 150"` — calling
/// `compute_hover` at a byte offset INSIDE the `expr` string must
/// return Some + markdown that mentions the AND-of-2 structure.
#[test]
fn h_s1_hover_on_check_expr_real_surface() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();
    let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age_valid","expr":"age > 0 AND age < 150"}]}"#;

    let tree = pool
        .parse(src, DocumentFormat::Json)
        .expect("hover JSON parses");

    // Cursor sits on the `>` operator of `age > 0` — well inside the expr body.
    let expr_inner_start =
        src.find(r#""expr":"age > 0"#).expect("expr present") + r#""expr":""#.len();
    let cursor = expr_inner_start + "age > 0".find('>').unwrap();
    assert_eq!(
        slice(src, cursor..cursor + 1),
        ">",
        "cursor byte must be on `>` operator"
    );

    let hover = compute_hover(src, DocumentFormat::Json, Some(&tree), &idx, &docs, cursor)
        .expect("hover should resolve inside CHECK expr");

    println!("=== HOVER (h_s1) ===");
    println!(
        "cursor byte: {cursor}  slice: {:?}",
        slice(src, cursor..cursor + 1)
    );
    println!(
        "anchor byte_range: {:?} -> {:?}",
        hover.byte_range,
        slice(src, hover.byte_range.clone())
    );
    println!("--- markdown ---");
    println!("{}", hover.markdown);
    println!("--- /markdown ---");

    let lower = hover.markdown.to_ascii_lowercase();
    assert!(
        lower.contains("and"),
        "hover markdown should describe the AND-of-2 structure; got: {}",
        hover.markdown
    );
    assert!(
        hover.markdown.contains("age"),
        "hover markdown should mention the `age` column; got: {}",
        hover.markdown
    );
}

/// I-S1: integer column `age` + `expr: "age > 0"` — calling
/// `compute_inlay_hints` must produce a hint with a label that
/// contains "integer", anchored at the byte offset right AFTER the
/// `age` identifier INSIDE the expr string.
#[test]
fn i_s1_inlay_hint_on_check_expr_real_surface() {
    let pool = ParserPool::new();
    let src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false,"primary_key":true},{"name":"age","type":"integer","nullable":false}],"constraints":[{"type":"check","name":"chk_age","expr":"age > 0"}]}"#;

    let tree = pool
        .parse(src, DocumentFormat::Json)
        .expect("inlay JSON parses");

    let hints = compute_inlay_hints(src, Some(&tree), 0..src.len());

    // The hint must anchor at the END of `age` INSIDE the expr literal.
    let expr_field = r#""expr":"age > 0""#;
    let expr_field_start = src.find(expr_field).expect("expr field present");
    let inner_start = expr_field_start + r#""expr":""#.len();
    let expected_anchor = inner_start + "age".len();

    println!("=== INLAY (i_s1) ===");
    for hint in &hints {
        // Show 3 bytes of context on each side of the anchor for clarity.
        let lo = hint.byte_offset.saturating_sub(3);
        let hi = (hint.byte_offset + 3).min(src.len());
        let ctx = src.get(lo..hi).unwrap_or("<oob>");
        println!(
            "{:?} @ byte {}  ctx={:?}  (anchor in slice {:?})",
            hint.label,
            hint.byte_offset,
            ctx,
            slice(
                src,
                hint.byte_offset..hint.byte_offset.min(src.len()).max(hint.byte_offset)
            )
        );
    }
    println!("expected CHECK-expr anchor: byte {expected_anchor}");

    let check_hint = hints
        .iter()
        .find(|h| h.byte_offset == expected_anchor)
        .unwrap_or_else(|| {
            panic!(
                "expected a CHECK-expr inlay hint at byte_offset {expected_anchor}; got: {hints:?}"
            )
        });
    assert!(
        check_hint.label.contains("integer"),
        "expected inlay label to contain `integer`, got: {:?}",
        check_hint.label
    );

    // Also assert at least one column-flag inlay still exists (PK on id),
    // so we know the new CHECK-expr emission did NOT replace prior hints.
    let id_col_start = src.find(r#"{"name":"id""#).expect("id column present");
    let pk_anchor = id_col_start + 1;
    assert!(
        hints
            .iter()
            .any(|h| h.byte_offset == pk_anchor && h.label.contains("PK")),
        "PK flag inlay must still be present at byte {pk_anchor}; got: {hints:?}"
    );
}

/// CMP-S1: cursor at the start of an empty CHECK `expr: ""` must offer
/// every declared column (id / age / name) via `compute_completion`.
/// CMP-S3: cursor after partial `ag` must offer `age` with a
/// `replace_range_bytes` that EXACTLY covers the two bytes of `ag` in
/// the source — proven by source-slicing the range.
#[test]
fn cmp_s1_and_s3_completion_in_check_expr_real_surface() {
    let pool = ParserPool::new();
    let idx = WorkspaceIndex::new();
    let docs = DocumentStore::new();

    // -- CMP-S1: empty `expr: ""` --------------------------------------
    let s1_src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false},{"name":"age","type":"integer","nullable":false},{"name":"name","type":"text","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":""}]}"#;
    let s1_tree = pool
        .parse(s1_src, DocumentFormat::Json)
        .expect("CMP-S1 source parses");
    let s1_cursor = s1_src.find(r#""expr":"""#).expect("empty expr present") + r#""expr":""#.len();
    let s1_items = compute_completion(
        s1_src,
        DocumentFormat::Json,
        Some(&s1_tree),
        &idx,
        &docs,
        s1_cursor,
    );
    let s1_labels: Vec<&str> = s1_items.iter().map(|i| i.label.as_str()).collect();

    println!("=== COMPLETION start (cmp_s1) ===");
    println!("cursor byte: {s1_cursor}");
    println!("labels ({}): {:?}", s1_labels.len(), s1_labels);

    for expected in ["id", "age", "name"] {
        assert!(
            s1_labels.contains(&expected),
            "CMP-S1 must offer column `{expected}`; got labels: {s1_labels:?}"
        );
    }

    // -- CMP-S3: partial `expr: "ag"` ---------------------------------
    let s3_src = r#"{"name":"users","columns":[{"name":"id","type":"integer","nullable":false},{"name":"age","type":"integer","nullable":false},{"name":"name","type":"text","nullable":false}],"constraints":[{"type":"check","name":"chk","expr":"ag"}]}"#;
    let s3_tree = pool
        .parse(s3_src, DocumentFormat::Json)
        .expect("CMP-S3 source parses");
    let s3_literal_start = s3_src
        .find(r#""ag""#)
        .expect("partial `ag` literal present");
    let s3_cursor = s3_literal_start + 1 + "ag".len();
    let s3_items = compute_completion(
        s3_src,
        DocumentFormat::Json,
        Some(&s3_tree),
        &idx,
        &docs,
        s3_cursor,
    );
    let age = s3_items
        .iter()
        .find(|i| i.label == "age")
        .expect("CMP-S3 must offer `age` for partial `ag`");
    let range = age
        .replace_range_bytes
        .as_ref()
        .expect("CMP-S3 `age` must carry replace_range_bytes");
    let replaced_slice = slice(s3_src, range.clone());

    println!("=== COMPLETION partial (cmp_s3) ===");
    println!("cursor byte: {s3_cursor}");
    println!(
        "label={:?} insert={:?} replace_range={:?} slice={:?}",
        age.label, age.insert_text, range, replaced_slice
    );

    assert_eq!(age.insert_text.as_deref(), Some("age"));
    assert_eq!(
        replaced_slice, "ag",
        "CMP-S3 replace_range_bytes must cover EXACTLY the partial SQL token `ag`"
    );
    let expected_range = (s3_literal_start + 1)..(s3_literal_start + 1 + "ag".len());
    assert_eq!(
        *range, expected_range,
        "CMP-S3 replace_range bytes mismatch"
    );
}
