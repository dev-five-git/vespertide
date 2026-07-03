//! Sort raw tokens by document position and delta-encode them into the
//! LSP wire format (`Vec<SemanticToken>` where each entry's `delta_line`
//! / `delta_start` is relative to the previous token, per the LSP §3.16
//! semanticTokens protocol).

use lsp_textdocument::FullTextDocument;
use tower_lsp_server::ls_types::SemanticToken;

use super::RawToken;

/// Convert a flat list of raw tokens (positioned by UTF-8 byte range)
/// into the delta-encoded LSP wire representation.
///
/// Invariants enforced:
///   * Multi-line tokens are dropped — LSP semantic tokens forbid them
///     (a single span MUST fit on one line).
///   * Output is sorted by `(line, character_utf16)`. Ties are stable
///     (insertion order from `tokens`), which can matter when two
///     classifiers report the same span — we keep the FIRST.
///   * Token lengths are measured in UTF-16 code units, not bytes —
///     `lsp-textdocument` handles that conversion.
#[must_use]
pub fn encode(tokens: &mut [RawToken], doc: &FullTextDocument) -> Vec<SemanticToken> {
    // Resolve every byte range to (line, start_utf16, length_utf16) once.
    // Drops zero-length tokens and multi-line spans up front.
    let mut resolved: Vec<Resolved> = tokens
        .iter()
        .filter_map(|t| Resolved::from_raw(t, doc))
        .collect();

    // Sort by start position. Ties preserved (stable sort) so consistent
    // classifier ordering wins.
    resolved.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.start.cmp(&b.start)));

    // Drop *exact* duplicate spans (same line + start + length) — they
    // would all collapse to delta_line=0/delta_start=0/length=L which
    // is meaningless and visually overlaps. Keep the first occurrence.
    resolved.dedup_by(|a, b| a.line == b.line && a.start == b.start && a.length == b.length);

    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;
    let mut out: Vec<SemanticToken> = Vec::with_capacity(resolved.len());

    for r in resolved {
        let delta_line = r.line - prev_line;
        let delta_start = if delta_line == 0 {
            // Same line: relative to the previous token's start. The
            // sort above guarantees `r.start >= prev_start`.
            r.start - prev_start
        } else {
            // New line: delta_start is absolute (relative to column 0).
            r.start
        };
        out.push(SemanticToken {
            delta_line,
            delta_start,
            length: r.length,
            token_type: r.token_type,
            token_modifiers_bitset: r.token_modifiers,
        });
        prev_line = r.line;
        prev_start = r.start;
    }

    out
}

#[derive(Debug, Clone, Copy)]
struct Resolved {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    token_modifiers: u32,
}

impl Resolved {
    fn from_raw(raw: &RawToken, doc: &FullTextDocument) -> Option<Self> {
        if raw.byte_range.end <= raw.byte_range.start {
            return None;
        }
        let start_byte = u32::try_from(raw.byte_range.start).ok()?;
        let end_byte = u32::try_from(raw.byte_range.end).ok()?;
        let start_pos = doc.position_at(start_byte);
        let end_pos = doc.position_at(end_byte);
        // LSP forbids multi-line tokens. Drop instead of fabricating
        // splits — classifiers should not emit them in the first place.
        if start_pos.line != end_pos.line {
            return None;
        }
        let length = end_pos.character.checked_sub(start_pos.character)?;
        if length == 0 {
            return None;
        }
        Some(Self {
            line: start_pos.line,
            start: start_pos.character,
            length,
            token_type: raw.token_type,
            token_modifiers: raw.token_modifiers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> FullTextDocument {
        FullTextDocument::new("json".to_string(), 1, text.to_string())
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let mut tokens: Vec<RawToken> = vec![];
        let encoded = encode(&mut tokens, &doc("{}"));
        assert!(encoded.is_empty());
    }

    #[test]
    fn single_line_tokens_use_relative_start_within_line() {
        let text = r#"{"name":"a","type":"int"}"#;
        let doc = doc(text);

        // Two tokens on the same line:
        //   "a"   at bytes 8..11   → length 3 utf16 codeunits
        //   "int" at bytes 19..24  → length 5 utf16 codeunits
        let a = RawToken {
            byte_range: 8..11,
            token_type: 0,
            token_modifiers: 0,
        };
        let i = RawToken {
            byte_range: 19..24,
            token_type: 1,
            token_modifiers: 0,
        };
        let mut tokens = vec![a, i];
        let encoded = encode(&mut tokens, &doc);
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0].delta_line, 0);
        assert_eq!(encoded[0].delta_start, 8);
        assert_eq!(encoded[0].length, 3);
        // Second token relative to first: delta_start = 19 - 8 = 11.
        assert_eq!(encoded[1].delta_line, 0);
        assert_eq!(encoded[1].delta_start, 11);
        assert_eq!(encoded[1].length, 5);
    }

    #[test]
    fn line_change_resets_delta_start_to_absolute() {
        let text = "a\n  b";
        let doc = doc(text);

        let first = RawToken {
            byte_range: 0..1, // "a" on line 0
            token_type: 0,
            token_modifiers: 0,
        };
        let second = RawToken {
            byte_range: 4..5, // "b" on line 1 col 2
            token_type: 1,
            token_modifiers: 0,
        };
        let mut tokens = vec![first, second];
        let encoded = encode(&mut tokens, &doc);
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[1].delta_line, 1);
        assert_eq!(encoded[1].delta_start, 2, "absolute when line changes");
    }

    #[test]
    fn multi_line_tokens_are_rejected() {
        let text = "abc\ndef";
        let doc = doc(text);
        let span = RawToken {
            byte_range: 0..7, // spans the newline
            token_type: 0,
            token_modifiers: 0,
        };
        let mut tokens = vec![span];
        let encoded = encode(&mut tokens, &doc);
        assert!(encoded.is_empty(), "multi-line tokens must be dropped");
    }

    #[test]
    fn unsorted_input_is_sorted_before_delta_encoding() {
        let text = r#"{"a":1,"b":2}"#;
        let doc = doc(text);
        // Provide tokens in REVERSE order — encode must sort first.
        let later = RawToken {
            byte_range: 7..10, // "b"
            token_type: 0,
            token_modifiers: 0,
        };
        let earlier = RawToken {
            byte_range: 1..4, // "a"
            token_type: 0,
            token_modifiers: 0,
        };
        let mut tokens = vec![later, earlier];
        let encoded = encode(&mut tokens, &doc);
        assert_eq!(encoded.len(), 2);
        // Earlier token reported first; later is delta from it.
        assert_eq!(encoded[0].delta_start, 1);
        assert!(encoded[1].delta_start > 0);
    }

    #[test]
    fn duplicate_spans_are_deduplicated() {
        let text = r#""x""#;
        let doc = doc(text);
        let twice = RawToken {
            byte_range: 0..3,
            token_type: 0,
            token_modifiers: 0,
        };
        let mut tokens = vec![twice.clone(), twice];
        let encoded = encode(&mut tokens, &doc);
        assert_eq!(encoded.len(), 1);
    }

    #[test]
    fn cjk_lengths_use_utf16_code_units() {
        // "도서" is 2 chars, 6 UTF-8 bytes, 2 UTF-16 code units.
        let text = "도서";
        let doc = doc(text);
        let token = RawToken {
            byte_range: 0..6,
            token_type: 0,
            token_modifiers: 0,
        };
        let mut tokens = vec![token];
        let encoded = encode(&mut tokens, &doc);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].length, 2, "length must be in UTF-16 code units");
    }

    #[test]
    fn raw_token_resolves_accented_byte_range_to_utf16_position() {
        let doc = doc("éx");
        let mut tokens = vec![RawToken {
            byte_range: 0..2,
            token_type: 2,
            token_modifiers: 0,
        }];

        let encoded = encode(&mut tokens, &doc);

        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].delta_line, 0);
        assert_eq!(encoded[0].delta_start, 0);
        assert_eq!(
            encoded[0].length, 1,
            "two UTF-8 bytes for é are one UTF-16 code unit"
        );
    }

    #[test]
    fn zero_length_tokens_are_rejected_before_encoding() {
        let mut tokens = vec![RawToken {
            byte_range: 1..1,
            token_type: 0,
            token_modifiers: 0,
        }];

        assert!(encode(&mut tokens, &doc("abc")).is_empty());
    }

    #[test]
    fn sub_character_ranges_that_map_to_zero_utf16_width_are_rejected() {
        let text = "🚀";
        let mut tokens = vec![RawToken {
            byte_range: 1..2,
            token_type: 0,
            token_modifiers: 0,
        }];

        assert!(encode(&mut tokens, &doc(text)).is_empty());
    }
}
