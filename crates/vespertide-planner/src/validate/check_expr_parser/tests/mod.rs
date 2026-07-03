use super::*;
use rstest::rstest;

#[test]
fn lex_simple_compare() {
    let expr = "age > 0";
    let tokens = lex_check_expr(expr);

    assert_eq!(
        tokens,
        vec![
            CheckToken {
                kind: CheckTokenKind::Column,
                span: 0..3
            },
            CheckToken {
                kind: CheckTokenKind::Operator,
                span: 4..5
            },
            CheckToken {
                kind: CheckTokenKind::Number,
                span: 6..7
            },
        ]
    );
    assert_eq!(&expr[tokens[0].span.clone()], "age");
    assert_eq!(&expr[tokens[1].span.clone()], ">");
    assert_eq!(&expr[tokens[2].span.clone()], "0");
}

#[test]
fn lex_duplicate_column_distinct_spans() {
    let expr = "age > 0 AND age < 150";
    let tokens = lex_check_expr(expr);
    let column_spans: Vec<_> = tokens
        .iter()
        .filter(|token| token.kind == CheckTokenKind::Column)
        .map(|token| token.span.clone())
        .collect();

    assert_eq!(tokens.len(), 7);
    assert_eq!(column_spans.as_slice(), &[0..3, 12..15]);
    assert_ne!(column_spans[0], column_spans[1]);
    assert_eq!(&expr[column_spans[0].clone()], "age");
    assert_eq!(&expr[column_spans[1].clone()], "age");
}

#[test]
fn lex_string_literal() {
    let expr = "status = 'active'";
    let tokens = lex_check_expr(expr);
    let texts: Vec<_> = tokens
        .iter()
        .map(|token| &expr[token.span.clone()])
        .collect();

    assert_eq!(
        tokens,
        vec![
            CheckToken {
                kind: CheckTokenKind::Column,
                span: 0..6
            },
            CheckToken {
                kind: CheckTokenKind::Operator,
                span: 7..8
            },
            CheckToken {
                kind: CheckTokenKind::String,
                span: 9..17
            },
        ]
    );
    assert_eq!(texts.as_slice(), &["status", "=", "'active'"]);
}

#[test]
fn lex_between() {
    let expr = "age BETWEEN 0 AND 150";
    let tokens = lex_check_expr(expr);
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    let texts: Vec<_> = tokens
        .iter()
        .map(|token| &expr[token.span.clone()])
        .collect();

    assert_eq!(
        kinds.as_slice(),
        &[
            CheckTokenKind::Column,
            CheckTokenKind::Keyword,
            CheckTokenKind::Number,
            CheckTokenKind::Keyword,
            CheckTokenKind::Number,
        ]
    );
    assert_eq!(texts.as_slice(), &["age", "BETWEEN", "0", "AND", "150"]);
}

#[test]
fn lex_in_list() {
    let expr = "status IN ('a', 'b')";
    let tokens = lex_check_expr(expr);
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    let texts: Vec<_> = tokens
        .iter()
        .map(|token| &expr[token.span.clone()])
        .collect();

    assert_eq!(
        kinds.as_slice(),
        &[
            CheckTokenKind::Column,
            CheckTokenKind::Keyword,
            CheckTokenKind::Punctuation,
            CheckTokenKind::String,
            CheckTokenKind::Punctuation,
            CheckTokenKind::String,
            CheckTokenKind::Punctuation,
        ]
    );
    assert_eq!(
        texts.as_slice(),
        &["status", "IN", "(", "'a'", ",", "'b'", ")"]
    );
}

#[test]
fn lex_is_null() {
    let expr = "deleted_at IS NULL";
    let tokens = lex_check_expr(expr);
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    let texts: Vec<_> = tokens
        .iter()
        .map(|token| &expr[token.span.clone()])
        .collect();

    assert_eq!(
        kinds.as_slice(),
        &[
            CheckTokenKind::Column,
            CheckTokenKind::Keyword,
            CheckTokenKind::Keyword,
        ]
    );
    assert_eq!(texts.as_slice(), &["deleted_at", "IS", "NULL"]);
}

#[test]
fn lex_unparseable_returns_empty() {
    assert!(lex_check_expr("status = 'unterminated").is_empty());
}

#[test]
fn lex_spans_are_byte_accurate() {
    let expr = "age >= 100";
    let tokens = lex_check_expr(expr);

    assert_eq!(tokens[1].span, 4..6);
    assert_eq!(&expr[tokens[1].span.clone()], ">=");
    assert_eq!(tokens[2].span, 7..10);
    assert_eq!(&expr[tokens[2].span.clone()], "100");
}

#[test]
fn lex_single_greater_than_operator_span_is_byte_accurate() {
    let expr = "age > 100";
    let tokens = lex_check_expr(expr);

    assert_eq!(tokens[1].kind, CheckTokenKind::Operator);
    assert_eq!(tokens[1].span, 4..5);
    assert_eq!(&expr[tokens[1].span.clone()], ">");
}

#[test]
fn empty_input_is_unparseable() {
    assert!(matches!(parse(""), CheckExpr::Unparseable));
    assert!(matches!(parse("   "), CheckExpr::Unparseable));
}

#[rstest]
#[case::gt("age > 0", "age", Op::Gt, Literal::Integer(0))]
#[case::bare_gt_age_eighteen("age > 18", "age", Op::Gt, Literal::Integer(18))]
#[case::gt_no_whitespace("age>18", "age", Op::Gt, Literal::Integer(18))]
#[case::ge("age >= 1", "age", Op::Ge, Literal::Integer(1))]
#[case::lt("amount < 100", "amount", Op::Lt, Literal::Integer(100))]
#[case::le("amount <= 100", "amount", Op::Le, Literal::Integer(100))]
#[case::eq("role = 'user'", "role", Op::Eq, Literal::String("'user'".into()))]
#[case::ne_iso("amount <> 0", "amount", Op::Ne, Literal::Integer(0))]
#[case::ne_bang("amount != 0", "amount", Op::Ne, Literal::Integer(0))]
fn simple_compare_parses(
    #[case] input: &str,
    #[case] expected_column: &str,
    #[case] expected_op: Op,
    #[case] expected_value: Literal,
) {
    let parsed = parse(input);
    let CheckExpr::Compare { column, op, value } = parsed else {
        panic!("expected Compare, got {parsed:?}");
    };
    assert_eq!(column, expected_column);
    assert_eq!(op, expected_op);
    assert_eq!(value, expected_value);
}

#[test]
fn in_list_parses() {
    let parsed = parse("status IN ('active', 'inactive', 'pending')");
    let CheckExpr::In {
        column,
        values,
        negated,
    } = parsed
    else {
        panic!("expected In");
    };
    assert_eq!(column, "status");
    assert!(!negated);
    assert_eq!(values.len(), 3);
}

#[test]
fn not_in_list_parses() {
    let parsed = parse("status NOT IN ('archived', 'deleted')");
    let CheckExpr::In {
        column, negated, ..
    } = parsed
    else {
        panic!("expected In with negated");
    };
    assert_eq!(column, "status");
    assert!(negated);
}

#[test]
fn between_parses() {
    let parsed = parse("age BETWEEN 0 AND 150");
    let CheckExpr::Between {
        column,
        low,
        high,
        negated,
    } = parsed
    else {
        panic!("expected Between, got {parsed:?}");
    };
    assert_eq!(column, "age");
    assert_eq!(low, Literal::Integer(0));
    assert_eq!(high, Literal::Integer(150));
    assert!(!negated);
}

#[test]
fn not_between_parses() {
    let parsed = parse("age NOT BETWEEN 0 AND 17");
    let CheckExpr::Between { negated, .. } = parsed else {
        panic!("expected Between");
    };
    assert!(negated);
}

#[test]
fn is_null_parses() {
    assert!(matches!(
        parse("deleted_at IS NULL"),
        CheckExpr::IsNull { negated: false, .. }
    ));
    assert!(matches!(
        parse("deleted_at IS NOT NULL"),
        CheckExpr::IsNull { negated: true, .. }
    ));
}

#[test]
fn and_composition_parses() {
    let parsed = parse("age > 0 AND age < 150");
    let CheckExpr::And(parts) = parsed else {
        panic!("expected And");
    };
    assert_eq!(parts.len(), 2);
}

#[test]
fn or_composition_parses() {
    let parsed = parse("status = 'a' OR status = 'b'");
    let CheckExpr::Or(parts) = parsed else {
        panic!("expected Or");
    };
    assert_eq!(parts.len(), 2);
}

#[test]
fn precedence_and_over_or() {
    // `a OR b AND c` should be `a OR (b AND c)`.
    let parsed = parse("x > 0 OR x < 100 AND x > 50");
    let CheckExpr::Or(or_parts) = parsed else {
        panic!("expected Or at top");
    };
    assert_eq!(or_parts.len(), 2);
    // Second OR branch is an AND.
    assert!(matches!(or_parts[1], CheckExpr::And(_)));
}

#[test]
fn parentheses_override_precedence() {
    let parsed = parse("(x > 0 OR x < 100) AND x > 50");
    let CheckExpr::And(and_parts) = parsed else {
        panic!("expected And at top");
    };
    assert_eq!(and_parts.len(), 2);
    assert!(matches!(and_parts[0], CheckExpr::Or(_)));
}

#[test]
fn case_insensitive_keywords() {
    assert!(matches!(
        parse("age between 0 and 10"),
        CheckExpr::Between { .. }
    ));
    assert!(matches!(
        parse("col is null"),
        CheckExpr::IsNull { negated: false, .. }
    ));
    assert!(matches!(parse("a > 0 and b < 0"), CheckExpr::And(_)));
}

#[test]
fn function_call_is_unparseable() {
    assert!(matches!(parse("LOWER(name) = 'x'"), CheckExpr::Unparseable));
    assert!(matches!(parse("LENGTH(name) > 0"), CheckExpr::Unparseable));
}

#[test]
fn column_to_column_is_unparseable() {
    // `a > b` — b is an identifier, not a literal.
    assert!(matches!(parse("a > b"), CheckExpr::Unparseable));
}

#[test]
fn empty_in_list_is_unparseable() {
    assert!(matches!(parse("col IN ()"), CheckExpr::Unparseable));
}

#[test]
fn pg_cast_is_unparseable() {
    assert!(matches!(parse("col::int > 0"), CheckExpr::Unparseable));
}

#[test]
fn trailing_garbage_is_unparseable() {
    assert!(matches!(parse("age > 0 garbage"), CheckExpr::Unparseable));
}

#[test]
fn or_with_unparseable_second_operand_is_unparseable() {
    // First operand parses, but the operand after `OR` is column-to-column
    // (unparseable). The `parse_or` loop must propagate Unparseable rather
    // than build a partial Or.
    assert!(matches!(parse("a > 0 OR x > b"), CheckExpr::Unparseable));
}

#[test]
fn and_with_unparseable_second_operand_is_unparseable() {
    // First operand parses, but the operand after `AND` is column-to-column
    // (unparseable). The `parse_and` loop must propagate Unparseable.
    assert!(matches!(parse("a > 0 AND x > b"), CheckExpr::Unparseable));
}

// A bare operator at end-of-input must not read `bytes[i+1]` out of bounds.
// Pins the two-char-lookahead guard `i + 1 < bytes.len()` (a `-`/`*`/`<=`
// mutant would index past the buffer and panic).
#[test]
fn trailing_operator_at_eof_is_unparseable_not_panic() {
    assert!(matches!(parse("age >"), CheckExpr::Unparseable));
}

// Incomplete exponent at EOF in a SIGNED literal slot must not read the
// exponent-sign byte out of bounds. Pins the `i < bytes.len() && ...` guard
// in the signed-number branch (`<=` / `||` mutants index past the buffer).
#[test]
fn incomplete_signed_exponent_at_eof_is_unparseable_not_panic() {
    assert!(matches!(parse("x = -1e"), CheckExpr::Unparseable));
}

// Same guard in the UNSIGNED-number branch.
#[test]
fn incomplete_unsigned_exponent_at_eof_is_unparseable_not_panic() {
    assert!(matches!(parse("x = 1e"), CheckExpr::Unparseable));
}

// 70 sequential parenthesized groups never nest deeper than 1, so depth must
// return to 0 after each. Pins `self.depth -= 1` in parse_atom: a `+=` or `/=`
// mutant leaks depth, tripping the MAX_CHECK_EXPR_DEPTH (64) guard and
// wrongly rejecting this valid expression.
#[test]
fn many_sequential_paren_groups_do_not_leak_depth() {
    let expr = std::iter::repeat_n("(c > 0)", 70)
        .collect::<Vec<_>>()
        .join(" OR ");
    assert!(
        !matches!(parse(&expr), CheckExpr::Unparseable),
        "70 flat OR groups must parse; depth must not leak"
    );
}

#[rstest]
#[case::null("col = NULL", Literal::Null)]
#[case::bool_true("col = TRUE", Literal::Bool(true))]
#[case::bool_false("col = FALSE", Literal::Bool(false))]
fn null_and_bool_literals(#[case] input: &str, #[case] expected: Literal) {
    let CheckExpr::Compare { value, .. } = parse(input) else {
        panic!("expected Compare");
    };
    assert_eq!(value, expected);
}

#[test]
fn float_literal() {
    assert!(matches!(
        parse("ratio > 0.5"),
        CheckExpr::Compare {
            value: Literal::Float(_),
            ..
        }
    ));
    assert!(matches!(
        parse("ratio > -0.5"),
        CheckExpr::Compare {
            value: Literal::Float(_),
            ..
        }
    ));
}

#[test]
fn scientific_notation() {
    assert!(matches!(
        parse("big > 1.5e3"),
        CheckExpr::Compare {
            value: Literal::Float(_),
            ..
        }
    ));
}

#[test]
fn nested_parens_in_and_or() {
    let parsed = parse("((a > 0 AND b > 0) OR (a < 0 AND b < 0))");
    assert!(matches!(parsed, CheckExpr::Or(_)));
}

#[test]
fn deeply_nested_parens_does_not_stack_overflow() {
    let expr = format!("{}age > 0{}", "(".repeat(5000), ")".repeat(5000));

    assert!(matches!(parse(&expr), CheckExpr::Unparseable));
}

#[test]
fn deeply_nested_not_does_not_overflow() {
    let expr = format!("{}age > 0", "NOT ".repeat(5000));

    assert!(matches!(parse(&expr), CheckExpr::Unparseable));
}

#[test]
fn moderate_nesting_still_parses() {
    assert!(matches!(parse("((age > 0))"), CheckExpr::Compare { .. }));
}

#[test]
fn unterminated_string_is_unparseable() {
    assert!(matches!(
        parse("col = 'unterminated"),
        CheckExpr::Unparseable
    ));
}

#[test]
fn doubled_quote_in_string_literal() {
    let parsed = parse("col = 'it''s'");
    let CheckExpr::Compare {
        value: Literal::String(s),
        ..
    } = parsed
    else {
        panic!("expected string compare");
    };
    assert_eq!(s, "'it''s'");
}

// -- matches_for_column shim tests (F86 / F4 compatibility) --------

#[test]
fn matches_for_column_simple_op() {
    assert!(matches_for_column("age > 0", "age"));
    assert!(!matches_for_column("age > 0", "other"));
}

#[test]
fn matches_for_column_in() {
    assert!(matches_for_column("status IN ('a', 'b')", "status"));
    assert!(!matches_for_column("status IN ('a', 'b')", "other"));
}

#[test]
fn matches_for_column_negated_in_not_matched() {
    // F86 only handles positive IN; negated forms fall outside
    // its evaluation contract.
    assert!(!matches_for_column("status NOT IN ('a', 'b')", "status"));
}

#[test]
fn matches_for_column_compound_not_matched() {
    // Compound expressions can't be projected back to a single
    // F86-shaped predicate against the given column.
    assert!(!matches_for_column("age > 0 AND age < 150", "age"));
}

// =====================================================================
// COVERAGE-CLOSURE TESTS — target specific lines flagged uncovered.
// Each #[rstest] case crafts an input that exercises a single lexer /
// parser arm; assertions on the returned CheckExpr / token kinds /
// extract_simple_column_check projection lock the behaviour without
// changing production code.
// =====================================================================

// Lexer: comma + paren punctuation spans (L296-302 arm)
#[test]
fn lex_comma_token_span_byte_accurate() {
    let expr = "x IN (1,2)";
    let tokens = lex_check_expr(expr);
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            CheckTokenKind::Column,
            CheckTokenKind::Keyword,
            CheckTokenKind::Punctuation,
            CheckTokenKind::Number,
            CheckTokenKind::Punctuation,
            CheckTokenKind::Number,
            CheckTokenKind::Punctuation,
        ]
    );
    // The middle comma sits at byte 7..8.
    let commas: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == CheckTokenKind::Punctuation && &expr[t.span.clone()] == ",")
        .collect();
    assert_eq!(commas.len(), 1);
    assert_eq!(commas[0].span, 7..8);
}

// Lexer: doubled-quote inside string literal pushes QuotedString
// span across both halves (L313-322 doubled-quote escape branch
// + L323-327 push).
#[test]
fn lex_doubled_single_quote_keeps_full_span() {
    let expr = "col = 'a''b'";
    let tokens = lex_check_expr(expr);
    let strings: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == CheckTokenKind::String)
        .collect();
    assert_eq!(strings.len(), 1);
    assert_eq!(&expr[strings[0].span.clone()], "'a''b'");
}

// Lexer: bare `!` rejected (L345 fallthrough `_ => return None`).
#[test]
fn lex_bare_bang_yields_unparseable() {
    assert!(lex_check_expr("a ! b").is_empty());
    assert!(matches!(parse("a ! b"), CheckExpr::Unparseable));
}

// Lexer: signed leading literal at start (L355-381 signed-number
// branch + L387 unsigned float dot).
#[test]
fn negative_int_at_start_of_in_list() {
    let parsed = parse("x IN (-1, -2)");
    let CheckExpr::In { values, .. } = parsed else {
        panic!("expected In, got {parsed:?}");
    };
    assert_eq!(values.len(), 2);
    assert_eq!(values[0], Literal::Integer(-1));
}

#[test]
fn unsigned_float_with_dot_in_expr() {
    let parsed = parse("ratio > 0.25");
    let CheckExpr::Compare {
        value: Literal::Float(f),
        ..
    } = parsed
    else {
        panic!("expected Float Compare");
    };
    assert!((f - 0.25).abs() < f64::EPSILON);
}

// Lexer: signed sign in a non-literal slot (`a-5`) yields None.
#[test]
fn signed_in_non_literal_slot_unparseable() {
    // After identifier `a`, `-` is not in a literal slot.
    assert!(matches!(parse("a - 5"), CheckExpr::Unparseable));
    assert!(matches!(parse("a -5"), CheckExpr::Unparseable));
}

// Lexer: scientific notation with negative exponent.
#[test]
fn float_with_negative_exponent() {
    let parsed = parse("ratio > 1.0e-2");
    let CheckExpr::Compare {
        value: Literal::Float(_),
        ..
    } = parsed
    else {
        panic!("expected Float, got {parsed:?}");
    };
}

#[test]
fn float_with_positive_exponent() {
    let parsed = parse("big > 2e+3");
    let CheckExpr::Compare {
        value: Literal::Float(_),
        ..
    } = parsed
    else {
        panic!("expected Float, got {parsed:?}");
    };
}

// Lexer: non-ASCII byte yields None (L419 wildcard fallthrough).
#[test]
fn non_ascii_byte_in_expr_is_unparseable() {
    assert!(matches!(parse("col = ñ"), CheckExpr::Unparseable));
}

// classify_word coverage: every keyword token (L463-474 word arms).
#[rstest]
#[case::kw_and("a > 0 AND b > 0")]
#[case::kw_or("a > 0 OR b > 0")]
#[case::kw_not_in("x NOT IN (1, 2)")]
#[case::kw_between("x BETWEEN 1 AND 10")]
#[case::kw_is_null("x IS NULL")]
#[case::kw_null_lit("x = NULL")]
#[case::kw_true_lit("x = TRUE")]
#[case::kw_false_lit("x = FALSE")]
fn classify_word_keywords_all_branches(#[case] input: &str) {
    // Each input forces classify_word to return Token::Keyword(_) for
    // the relevant reserved word; the parse simply must NOT be
    // Unparseable on AND/OR/NOT-IN/BETWEEN/IS, and a Compare otherwise.
    let parsed = parse(input);
    assert!(!matches!(parsed, CheckExpr::Unparseable));
}

// Parser: bump returns None at end of stream (L494-500 bump branch).
#[test]
fn predicate_truncated_after_ident_unparseable() {
    assert!(matches!(parse("only_ident"), CheckExpr::Unparseable));
}

// Parser: eat_keyword false branch (L502-509) — non-keyword at peek.
#[test]
fn parse_and_single_then_eof() {
    // Single compare returned (parse_and falls through to pop()).
    let parsed = parse("age > 0");
    assert!(matches!(parsed, CheckExpr::Compare { .. }));
}

// Parser: parse_or returns Unparseable when first parse_and is bad
// (L513-515).
#[test]
fn parse_or_first_unparseable_propagates() {
    assert!(matches!(parse("?? OR x > 0"), CheckExpr::Unparseable));
}

// Parser: parse_or returns Unparseable when later branch is bad
// (L519-521).
#[test]
fn parse_or_second_unparseable_propagates() {
    assert!(matches!(parse("x > 0 OR ??"), CheckExpr::Unparseable));
}

// Parser: parse_or returns Or with parts (L527).
#[test]
fn parse_or_with_three_branches() {
    let parsed = parse("a = 1 OR a = 2 OR a = 3");
    let CheckExpr::Or(parts) = parsed else {
        panic!("expected Or");
    };
    assert_eq!(parts.len(), 3);
}

// Parser: parse_and returns And with parts (L547).
#[test]
fn parse_and_with_three_branches() {
    let parsed = parse("a > 0 AND a < 100 AND a <> 50");
    let CheckExpr::And(parts) = parsed else {
        panic!("expected And");
    };
    assert_eq!(parts.len(), 3);
}

// Parser: parse_and unparseable after AND (L539-541).
#[test]
fn parse_and_second_branch_bad_unparseable() {
    assert!(matches!(parse("x > 0 AND ??"), CheckExpr::Unparseable));
}

// Parser: parse_not Unparseable inner (L554-556).
#[test]
fn parse_not_inner_unparseable() {
    assert!(matches!(parse("NOT ??"), CheckExpr::Unparseable));
}

// Parser: parse_not wraps inner (L557).
#[test]
fn parse_not_wraps_atom() {
    let parsed = parse("NOT (age > 0)");
    let CheckExpr::Not(inner) = parsed else {
        panic!("expected Not, got {parsed:?}");
    };
    assert!(matches!(*inner, CheckExpr::Compare { .. }));
}

// Parser: parse_atom non-LParen non-Ident leads to Unparseable
// (L583).
#[test]
fn parse_atom_starts_with_number_unparseable() {
    assert!(matches!(parse("42"), CheckExpr::Unparseable));
}

// Parser: parse_atom unmatched paren returns Unparseable (L577).
#[test]
fn parse_atom_missing_closing_paren_unparseable() {
    assert!(matches!(parse("(age > 0"), CheckExpr::Unparseable));
}

// Parser: parse_atom paren depth budget reached at MAX_CHECK_EXPR_DEPTH
// (L567-568) — already covered by deeply_nested but ensure direct path.
#[test]
fn parse_atom_inner_unparseable_yields_unparseable() {
    assert!(matches!(parse("(??)"), CheckExpr::Unparseable));
}

// Parser: predicate `NOT <op>` rejected (L595-596).
#[test]
fn predicate_negated_compare_unparseable() {
    assert!(matches!(parse("col NOT > 5"), CheckExpr::Unparseable));
}

// Parser: predicate IN without `(` (L610-611).
#[test]
fn predicate_in_without_paren_unparseable() {
    assert!(matches!(parse("col IN 1, 2"), CheckExpr::Unparseable));
}

// Parser: IN list with trailing junk (L628).
#[test]
fn predicate_in_with_junk_separator_unparseable() {
    assert!(matches!(parse("col IN (1 AND 2)"), CheckExpr::Unparseable));
}

// Parser: BETWEEN missing AND keyword (L646-647).
#[test]
fn predicate_between_without_and_unparseable() {
    assert!(matches!(parse("col BETWEEN 1 2"), CheckExpr::Unparseable));
}

// Parser: BETWEEN missing high literal (L649-650).
#[test]
fn predicate_between_missing_high_unparseable() {
    assert!(matches!(parse("col BETWEEN 1 AND"), CheckExpr::Unparseable));
}

// Parser: BETWEEN missing low literal (L643-644).
#[test]
fn predicate_between_missing_low_unparseable() {
    assert!(matches!(
        parse("col BETWEEN AND 10"),
        CheckExpr::Unparseable
    ));
}

// Parser: `col NOT IS NULL` rejected (L660-661).
#[test]
fn predicate_not_is_null_inverted_unparseable() {
    assert!(matches!(parse("col NOT IS NULL"), CheckExpr::Unparseable));
}

// Parser: IS without NULL keyword (L665-666).
#[test]
fn predicate_is_without_null_unparseable() {
    assert!(matches!(parse("col IS"), CheckExpr::Unparseable));
}

// Parser: predicate ending with stray token (L670).
#[test]
fn predicate_bare_ident_then_paren_unparseable() {
    assert!(matches!(parse("col (1)"), CheckExpr::Unparseable));
}

// Parser: try_take_literal None branch (L683).
#[test]
fn predicate_op_followed_by_non_literal_unparseable() {
    assert!(matches!(parse("col = AND"), CheckExpr::Unparseable));
}

// extract_simple_column_check: positive In branch (L727-731).
#[test]
fn extract_simple_in_for_target_column() {
    let parsed = parse("status IN ('a', 'b')");
    let result = extract_simple_column_check(&parsed, "status");
    assert!(matches!(result, Some(SimpleColumnCheck::In(ref v)) if v.len() == 2));
}

// extract_simple_column_check: column-mismatch In returns None.
#[test]
fn extract_simple_in_for_wrong_column_none() {
    let parsed = parse("status IN ('a')");
    assert!(extract_simple_column_check(&parsed, "other").is_none());
}

// extract_simple_column_check: negated In returns None.
#[test]
fn extract_simple_negated_in_none() {
    let parsed = parse("status NOT IN ('a', 'b')");
    assert!(extract_simple_column_check(&parsed, "status").is_none());
}

// extract_simple_column_check: AndOrNot tree fallthrough.
#[test]
fn extract_simple_non_simple_shape_none() {
    let parsed = parse("a > 0 AND b > 0");
    assert!(extract_simple_column_check(&parsed, "a").is_none());
}

// CheckExpr::Compare for wrong column returns None.
#[test]
fn extract_simple_compare_wrong_column_none() {
    let parsed = parse("a > 0");
    assert!(extract_simple_column_check(&parsed, "b").is_none());
}

// =====================================================================
// COVERAGE-CLOSURE W3 — signed scientific notation + parse_number_token
// fallthrough (L320-325, L387). The IN-empty arm (L549) is provably
// unreachable: parse_predicate's IN loop pushes a literal at every
// iteration before checking the trailing tokens, so `values.is_empty()`
// at L547 cannot hold; the guard exists only for documentation.
// =====================================================================

/// L319-326: signed scientific notation. `+` consumed inside the
/// scientific exponent's sign path (L321 `+`) plus the digit run.
#[rstest]
#[case::neg_pos_exp("x > -1.5e+10")]
#[case::neg_neg_exp("y > -2.5e-3")]
#[case::pos_pos_exp("x IN (+3e+5)")]
#[case::pos_neg_exp("x IN (+4e-7)")]
fn signed_scientific_notation_parses(#[case] input: &str) {
    let parsed = parse(input);
    assert!(
        !matches!(parsed, CheckExpr::Unparseable),
        "expected non-Unparseable, got {parsed:?}"
    );
}

/// L380-388: parse_number_token returns None for a raw `+` / `-` slice
/// (no digits after the sign). The signed branch consumes the `+` /
/// `-`, the digit/dot/e loop runs zero iterations, raw is "+" or "-".
/// `i64`/`f64` both reject this → None at L387 → tokenize fails →
/// `parse` returns `Unparseable`.
#[rstest]
#[case::lone_plus("x > +")]
#[case::lone_minus("x > -")]
fn lone_sign_in_literal_slot_unparseable(#[case] input: &str) {
    assert!(matches!(parse(input), CheckExpr::Unparseable));
}

/// L256-259: explicit per-token cover for the `b')'` arm of
/// `tokenize_spanned`. `lex_in_list` indirectly hits it, but this
/// direct assertion locks the (kind, span) for the closing paren so
/// future refactors that shift other arms cannot silently demote
/// `)` to a different token kind. Spans are byte-accurate.
#[rstest]
#[case::after_in_list("x IN (1, 2)", 10)] // `)` at byte 10
#[case::nested_parens("(((a > 0)))", 8)] // first `)` at byte 8
#[case::between_value("age BETWEEN (1) AND (5)", 14)] // first `)` at byte 14
fn lex_close_paren_emits_punctuation_at_byte_span(
    #[case] expr: &str,
    #[case] first_rparen_byte: usize,
) {
    let tokens = lex_check_expr(expr);
    let rparens: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == CheckTokenKind::Punctuation && &expr[t.span.clone()] == ")")
        .collect();
    assert!(
        !rparens.is_empty(),
        "expected at least one ')' Punctuation token, got: {tokens:?}"
    );
    assert_eq!(
        rparens[0].span,
        first_rparen_byte..first_rparen_byte + 1,
        "first ')' span mismatch for `{expr}`"
    );
}

/// Directly pins `is_literal_start_slot` for the exact token contexts
/// that allow a signed literal. This avoids relying on later parser
/// failure to prove the lexer reached each `Keyword(...)` match arm.
#[rstest]
#[case::start(Vec::new())]
#[case::after_operator(vec![spanned(Token::Op(Op::Eq))])]
#[case::after_lparen(vec![spanned(Token::LParen)])]
#[case::after_comma(vec![spanned(Token::Comma)])]
#[case::after_and(vec![spanned(Token::Keyword(Keyword::And))])]
#[case::after_or(vec![spanned(Token::Keyword(Keyword::Or))])]
#[case::after_not(vec![spanned(Token::Keyword(Keyword::Not))])]
#[case::after_between(vec![spanned(Token::Keyword(Keyword::Between))])]
#[case::after_in(vec![spanned(Token::Keyword(Keyword::In))])]
#[case::after_is(vec![spanned(Token::Keyword(Keyword::Is))])]
fn signed_literal_slot_accepts_all_documented_prefixes(#[case] tokens: Vec<SpannedToken>) {
    assert!(is_literal_start_slot(&tokens));
}

#[rstest]
#[case::after_identifier(vec![spanned(Token::Ident("age".into()))])]
#[case::after_number(vec![spanned(Token::Integer(1))])]
#[case::after_rparen(vec![spanned(Token::RParen)])]
#[case::after_null_keyword(vec![spanned(Token::Keyword(Keyword::Null))])]
fn signed_literal_slot_rejects_operand_like_prefixes(#[case] tokens: Vec<SpannedToken>) {
    assert!(!is_literal_start_slot(&tokens));
}

#[test]
fn classify_word_and_keyword_direct_branch() {
    assert_eq!(classify_word("AND"), Token::Keyword(Keyword::And));
    assert_eq!(classify_word("and"), Token::Keyword(Keyword::And));
}

#[test]
fn eat_keyword_false_branch_leaves_position_unchanged() {
    let mut parser = Parser {
        tokens: vec![Token::Keyword(Keyword::And)],
        pos: 0,
        depth: 0,
    };

    assert!(!parser.eat_keyword(Keyword::Or));
    assert_eq!(parser.pos, 0);
}

fn spanned(token: Token) -> SpannedToken {
    SpannedToken { token, span: 0..0 }
}

/// Same arm via the full `parse` pipeline — every parens-balanced
/// expression must reach the `b')'` branch at least once during
/// tokenization, and `parse` must NOT fold valid input to Unparseable.
#[rstest]
#[case::in_list("x IN (1, 2, 3)")]
#[case::between_with_parens("(age > 0) AND (age < 100)")]
#[case::nested("((x > 0))")]
fn parse_balanced_parens_does_not_fold_to_unparseable(#[case] expr: &str) {
    let parsed = parse(expr);
    assert!(
        !matches!(parsed, CheckExpr::Unparseable),
        "balanced-parens expression `{expr}` must parse, got: {parsed:?}"
    );
}
