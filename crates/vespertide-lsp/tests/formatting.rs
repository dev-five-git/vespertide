use vespertide_lsp::{DocumentFormat, format_text};

#[test]
fn json_minified_to_pretty() {
    let input = r#"{"name":"user","columns":[]}"#;
    let pretty = format_text(input, DocumentFormat::Json).unwrap();

    assert!(pretty.contains('\n'));
    assert!(pretty.contains("  "));
}

#[test]
fn already_pretty_idempotent_ish() {
    let pretty = "{\n  \"name\": \"user\"\n}\n";
    let result = format_text(pretty, DocumentFormat::Json).unwrap();

    let original: serde_json::Value = serde_json::from_str(pretty).unwrap();
    let formatted: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(original, formatted);
}
