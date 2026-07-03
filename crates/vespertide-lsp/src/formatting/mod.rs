//! Formatting provider — pretty-print JSON and YAML model files.

use crate::parser::DocumentFormat;

/// Pretty-print the document.
///
/// Returns `None` when parsing fails. Formatting is semantic-preserving for
/// the supported serde data model.
pub fn format_text(text: &str, format: DocumentFormat) -> Option<String> {
    match format {
        DocumentFormat::Json => format_json(text),
        DocumentFormat::Yaml => format_yaml(text),
    }
}

fn format_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
    serde::Serialize::serialize(&value, &mut serializer).ok()?;
    let mut out = String::from_utf8(buf).ok()?;
    ensure_trailing_newline(&mut out);
    Some(out)
}

fn format_yaml(text: &str) -> Option<String> {
    let value: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
    let mut out = serde_yaml::to_string(&value).ok()?;
    ensure_trailing_newline(&mut out);
    Some(out)
}

fn ensure_trailing_newline(text: &mut String) {
    if !text.ends_with('\n') {
        text.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn json_pretty_indents_2_spaces() {
        let text = r#"{"name":"user","columns":[{"name":"id","type":"integer"}]}"#;
        let pretty = format_json(text).unwrap();

        assert!(pretty.contains("  "));
        assert!(pretty.ends_with('\n'));

        let original: serde_json::Value = serde_json::from_str(text).unwrap();
        let formatted: serde_json::Value = serde_json::from_str(&pretty).unwrap();
        assert_eq!(original, formatted);
    }

    #[test]
    fn yaml_round_trip_preserved() {
        let text = "name: user\ncolumns: []\n";
        let pretty = format_yaml(text).unwrap();

        let original: serde_yaml::Value = serde_yaml::from_str(text).unwrap();
        let formatted: serde_yaml::Value = serde_yaml::from_str(&pretty).unwrap();
        assert_eq!(original, formatted);
    }

    #[test]
    fn invalid_json_returns_none() {
        assert!(format_json("{not json}").is_none());
    }

    #[rstest]
    #[case::json(r#"{"name":"u","columns":[]}"#, DocumentFormat::Json)]
    #[case::yaml("name: user\ncolumns: []\n", DocumentFormat::Yaml)]
    fn format_text_dispatches_by_document_format(
        #[case] text: &str,
        #[case] format: DocumentFormat,
    ) {
        let formatted = format_text(text, format).expect("format should succeed");

        assert!(formatted.ends_with('\n'));
    }
}
