//! UTF-16 (LSP) ↔ byte offset (tree-sitter/Rust) position conversions.
//!
//! Uses `lsp-textdocument`'s [`FullTextDocument`] which handles this correctly
//! (verified by rust-analyzer + nushell switching to it after ropey UTF-8 bugs).
//!
//! Also provides small bridges between `tower_lsp_server::ls_types` and the
//! upstream `lsp_types` crate. The two are structurally identical at the
//! position/range level but are distinct types because tower-lsp-server
//! maintains a fork; conversion happens at the I/O seam, never inside the
//! analysis engine.

use lsp_textdocument::FullTextDocument;
use tower_lsp_server::ls_types::Uri;

/// Convert an LSP `lsp_types::Position` to a UTF-8 byte offset.
#[must_use]
pub fn lsp_position_to_byte(doc: &FullTextDocument, pos: lsp_types::Position) -> usize {
    doc.offset_at(pos) as usize
}

/// Convert a UTF-8 byte offset to an LSP `lsp_types::Position`.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "lsp-textdocument accepts u32 byte offsets for UTF-16 positions; offsets are bounded by the opened document size"
)]
pub fn byte_to_lsp_position(doc: &FullTextDocument, byte_offset: usize) -> lsp_types::Position {
    doc.position_at(byte_offset as u32)
}

/// Bridge from tower-lsp-server's `ls_types::Position` to `lsp_types::Position`.
/// They're structurally identical but type-distinct (different crates).
#[must_use]
pub fn ls_to_lsp_position(p: tower_lsp_server::ls_types::Position) -> lsp_types::Position {
    lsp_types::Position {
        line: p.line,
        character: p.character,
    }
}

/// Bridge from `lsp_types::Position` to `ls_types::Position`.
#[must_use]
pub fn lsp_to_ls_position(p: lsp_types::Position) -> tower_lsp_server::ls_types::Position {
    tower_lsp_server::ls_types::Position {
        line: p.line,
        character: p.character,
    }
}

/// Bridge from tower-lsp-server's `ls_types::Range` to `lsp_types::Range`.
#[must_use]
pub fn ls_to_lsp_range(r: tower_lsp_server::ls_types::Range) -> lsp_types::Range {
    lsp_types::Range {
        start: ls_to_lsp_position(r.start),
        end: ls_to_lsp_position(r.end),
    }
}

/// Convert a `file://` URI into a local filesystem path.
///
/// Handles percent-encoding (VS Code sends `file:///c%3A/...`, Zed sends
/// `file:///C:/...`) and Windows drive-letter prefixes uniformly so the
/// resulting `PathBuf` can be compared, opened, and canonicalized.
#[must_use]
pub fn uri_to_path(uri: &Uri) -> Option<std::path::PathBuf> {
    let uri_text = uri.to_string();
    let raw = uri_text.strip_prefix("file://")?;
    let decoded = percent_decode(raw);

    let path = if cfg!(windows) {
        decoded
            .strip_prefix('/')
            .filter(|without_slash| has_windows_drive_prefix(without_slash))
            .unwrap_or(&decoded)
            .replace('/', std::path::MAIN_SEPARATOR_STR)
    } else {
        decoded
    };

    Some(std::path::PathBuf::from(path))
}

/// Decode `%XX` triplets into raw bytes, then re-validate as UTF-8.
/// Invalid sequences are left as-is so we never destroy unrelated text.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Lossy-convert to keep the function infallible; URIs we receive are
    // already supposed to be valid UTF-8 once decoded.
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_textdocument::FullTextDocument;
    use rstest::rstest;
    use std::str::FromStr;
    use tower_lsp_server::ls_types::Uri;

    fn doc(text: &str) -> FullTextDocument {
        FullTextDocument::new("json".to_string(), 1, text.to_string())
    }

    #[test]
    fn ascii_round_trip() {
        let d = doc("hello world");
        let pos = lsp_types::Position {
            line: 0,
            character: 6,
        };
        let byte = lsp_position_to_byte(&d, pos);
        assert_eq!(byte, 6);
        let pos2 = byte_to_lsp_position(&d, byte);
        assert_eq!(pos2, pos);
    }

    #[test]
    fn cjk_round_trip() {
        // "도서" = 2 chars, 6 bytes UTF-8, 2 UTF-16 code units (BMP).
        let d = doc("도서 test");
        let pos = lsp_types::Position {
            line: 0,
            character: 2,
        }; // after "도서"
        let byte = lsp_position_to_byte(&d, pos);
        assert_eq!(byte, 6); // 2 chars × 3 bytes each
    }

    #[test]
    fn emoji_round_trip() {
        // "🚀" = 1 char, 4 bytes UTF-8, 2 UTF-16 code units (surrogate pair).
        let d = doc("🚀test");
        let pos_after_emoji = lsp_types::Position {
            line: 0,
            character: 2,
        }; // 2 UTF-16 units
        let byte = lsp_position_to_byte(&d, pos_after_emoji);
        assert_eq!(byte, 4); // 4 UTF-8 bytes
    }

    #[test]
    fn multiline_position() {
        let d = doc("line one\nline two\nline three");
        let pos = lsp_types::Position {
            line: 1,
            character: 5,
        }; // "line " on line 2
        let byte = lsp_position_to_byte(&d, pos);
        assert_eq!(byte, "line one\nline ".len());
    }

    #[test]
    fn position_bridge_round_trip() {
        let p = lsp_types::Position {
            line: 5,
            character: 10,
        };
        let ls = lsp_to_ls_position(p);
        let back = ls_to_lsp_position(ls);
        assert_eq!(back, p);
    }

    #[test]
    fn range_bridge() {
        let ls = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position {
                line: 1,
                character: 2,
            },
            end: tower_lsp_server::ls_types::Position {
                line: 3,
                character: 4,
            },
        };
        let r = ls_to_lsp_range(ls);
        assert_eq!(r.start.line, 1);
        assert_eq!(r.start.character, 2);
        assert_eq!(r.end.line, 3);
        assert_eq!(r.end.character, 4);
    }

    /// Regression — VS Code on Windows sends `file:///c%3A/Users/...`.
    /// Without percent-decoding, `uri_to_path` produced `\c%3A\Users\...`
    /// which never matches anything on disk → `workspace_tables.refresh()`
    /// failed silently → every cross-file FK reported "table not found".
    #[test]
    #[cfg(windows)]
    fn uri_to_path_decodes_vscode_percent_encoded_drive_letter() {
        use std::str::FromStr;
        use tower_lsp_server::ls_types::Uri;

        let uri =
            Uri::from_str("file:///c%3A/Users/owjs3/Desktop/projects/vespertide/examples/app")
                .unwrap();
        let path = uri_to_path(&uri).expect("path");
        assert_eq!(
            path,
            std::path::PathBuf::from(r"C:\Users\owjs3\Desktop\projects\vespertide\examples\app"),
            "VS Code percent-encoded drive letter must decode to a real path"
        );
    }

    /// Plain `C:` (without percent-encoding) used by Zed, neovim, etc.
    /// must keep working as before.
    #[test]
    #[cfg(windows)]
    fn uri_to_path_handles_raw_drive_letter() {
        use std::str::FromStr;
        use tower_lsp_server::ls_types::Uri;

        let uri = Uri::from_str("file:///C:/Users/owjs3/Desktop").unwrap();
        let path = uri_to_path(&uri).expect("path");
        assert_eq!(path, std::path::PathBuf::from(r"C:\Users\owjs3\Desktop"));
    }

    #[test]
    fn uri_to_path_decodes_spaces_and_unicode() {
        use std::str::FromStr;
        use tower_lsp_server::ls_types::Uri;

        let uri = Uri::from_str("file:///tmp/with%20space/%ED%95%9C%EA%B8%80.json").unwrap();
        let path = uri_to_path(&uri).expect("path");
        let text = path.to_string_lossy();
        assert!(text.contains("with space"), "got: {text}");
        assert!(text.contains("한글.json"), "got: {text}");
    }

    #[test]
    fn percent_decode_leaves_invalid_triplets_intact() {
        assert_eq!(
            percent_decode("/tmp/bad%2Gname.json"),
            "/tmp/bad%2Gname.json"
        );
    }

    #[test]
    fn percent_decode_consumes_valid_triplets_and_continues_after_them() {
        assert_eq!(percent_decode("/tmp/%41%2fname.json"), "/tmp/A/name.json");
    }

    #[rstest]
    #[case::unencoded("file:///plain/path/file.json", &["plain", "file.json"])]
    #[case::lowercase_hex_drive("file:///c%3a/Users/test", &["Users"])]
    fn uri_to_path_decodes_file_uris(#[case] raw_uri: &str, #[case] expected_parts: &[&str]) {
        let uri = Uri::from_str(raw_uri).unwrap();
        let path = uri_to_path(&uri).expect("file URI should become a path");
        let text = path.to_string_lossy();

        for part in expected_parts {
            assert!(text.contains(part), "expected `{part}` in {text}");
        }
    }

    #[test]
    fn uri_to_path_returns_none_for_non_file_uri() {
        let uri = Uri::from_str("https://example.com/x").unwrap();

        assert!(uri_to_path(&uri).is_none());
    }

    #[test]
    fn position_lsp_to_byte_round_trip_on_second_line() {
        let doc = FullTextDocument::new("json".to_string(), 1, "ab\ncd".to_string());
        let pos = lsp_types::Position {
            line: 1,
            character: 1,
        };
        let byte = lsp_position_to_byte(&doc, pos);

        assert_eq!(byte_to_lsp_position(&doc, byte), pos);
    }

    #[test]
    fn has_windows_drive_prefix_detects_and_rejects() {
        // Tested directly: the only call site sits behind `if cfg!(windows)`,
        // so on non-Windows CI the function is otherwise never invoked.
        assert!(has_windows_drive_prefix("C:/Users/x"));
        assert!(has_windows_drive_prefix("d:/data"));
        assert!(!has_windows_drive_prefix("/usr/local"));
        assert!(!has_windows_drive_prefix("ab"));
        assert!(!has_windows_drive_prefix(""));
    }
}
