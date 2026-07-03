//! Tokenise CHECK constraint expressions for semantic highlighting.
//!
//! Bridges the planner's span-aware CHECK lexer
//! (`vespertide_planner::lex_check_expr`) to LSP `RawToken`s. Each
//! lexeme's span (relative to the expression text) is translated to
//! an absolute document byte range by adding `inner_start` — the byte
//! offset of the first character of the expression *inside* the
//! enclosing JSON/YAML string value (i.e. after the opening quote).

use vespertide_planner::{CheckTokenKind, lex_check_expr};

use super::RawToken;
use super::legend::TokenIdx;

/// Emit one `RawToken` per highlightable CHECK lexeme, with byte
/// ranges absolute to the source document. Punctuation (`( ) ,`) is
/// skipped. Malformed expressions (lexer returns empty) emit nothing.
pub(super) fn emit_check_expr_tokens(expr_text: &str, inner_start: usize, out: &mut Vec<RawToken>) {
    for token in lex_check_expr(expr_text) {
        let Some(token_type) = token_kind_to_idx(token.kind) else {
            continue;
        };
        let abs = (inner_start + token.span.start)..(inner_start + token.span.end);
        out.push(RawToken {
            byte_range: abs,
            token_type: token_type as u32,
            token_modifiers: 0,
        });
    }
}

fn token_kind_to_idx(kind: CheckTokenKind) -> Option<TokenIdx> {
    match kind {
        CheckTokenKind::Column => Some(TokenIdx::Property),
        CheckTokenKind::Keyword | CheckTokenKind::Operator => Some(TokenIdx::Keyword),
        CheckTokenKind::Number => Some(TokenIdx::Number),
        CheckTokenKind::String => Some(TokenIdx::String),
        CheckTokenKind::Punctuation => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::legend::TokenIdx;
    use super::*;

    #[test]
    fn punctuation_yields_no_token() {
        // `(age > 0)` lexes to `( age > 0 )` — the parens are Punctuation
        // and must be dropped, leaving exactly the 3 inner tokens.
        let mut out = Vec::new();
        emit_check_expr_tokens("(age > 0)", 10, &mut out);
        assert_eq!(out.len(), 3, "parens dropped, kept: {out:?}");
        // Absolute byte ranges = inner_start + span.start, all > 10.
        for tok in &out {
            assert!(tok.byte_range.start >= 10 && tok.byte_range.end > tok.byte_range.start);
        }
    }

    #[test]
    fn comma_punctuation_dropped_in_in_list() {
        let mut out = Vec::new();
        emit_check_expr_tokens("status IN ('a', 'b')", 0, &mut out);
        // Tokens: status(Property), IN(Keyword), 'a'(String), 'b'(String).
        // Punctuation: ( , ) — dropped.
        let types: Vec<u32> = out.iter().map(|t| t.token_type).collect();
        assert!(types.contains(&(TokenIdx::Property as u32)));
        assert!(types.contains(&(TokenIdx::Keyword as u32)));
        assert!(types.contains(&(TokenIdx::String as u32)));
        assert!(!out.iter().any(|t| {
            // No emitted token may cover a bare punctuation byte.
            let s = "status IN ('a', 'b')";
            let slice = &s[t.byte_range.clone()];
            matches!(slice, "(" | ")" | ",")
        }));
    }

    #[test]
    fn column_kind_maps_to_property() {
        let mut out = Vec::new();
        emit_check_expr_tokens("foo", 0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].token_type, TokenIdx::Property as u32);
    }

    #[test]
    fn keyword_and_operator_share_keyword_index() {
        let mut out = Vec::new();
        emit_check_expr_tokens("a AND b > 0", 0, &mut out);
        let keyword_count = out
            .iter()
            .filter(|t| t.token_type == TokenIdx::Keyword as u32)
            .count();
        // AND (Keyword) + > (Operator) both fold into TokenIdx::Keyword.
        assert_eq!(keyword_count, 2, "AND + > should yield 2 Keyword tokens");
    }

    #[test]
    fn string_literal_yields_string_token() {
        let mut out = Vec::new();
        emit_check_expr_tokens("status = 'active'", 0, &mut out);
        assert!(out.iter().any(|t| t.token_type == TokenIdx::String as u32));
    }

    #[test]
    fn empty_input_emits_nothing() {
        let mut out = Vec::new();
        emit_check_expr_tokens("", 0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn inner_start_offset_is_applied() {
        let mut out = Vec::new();
        emit_check_expr_tokens("x", 100, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].byte_range, 100..101);
    }
}
