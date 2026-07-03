use tempfile::tempdir;
use tower_lsp_server::ls_types::{
    CompletionItemKind as LspCompletionItemKind, CompletionTextEdit, Diagnostic,
    DiagnosticSeverity, Position, Range, SymbolKind as LspSymbolKind,
};

use super::harness::{make_service, uri};

#[rstest::rstest]
#[case::value(
    crate::completion::CompletionItemKind::Value,
    LspCompletionItemKind::VALUE
)]
#[case::property(
    crate::completion::CompletionItemKind::Property,
    LspCompletionItemKind::PROPERTY
)]
#[case::reference(
    crate::completion::CompletionItemKind::Reference,
    LspCompletionItemKind::REFERENCE
)]
#[case::snippet(
    crate::completion::CompletionItemKind::Snippet,
    LspCompletionItemKind::SNIPPET
)]
fn domain_to_lsp_maps_scalar_and_property_kinds(
    #[case] kind: crate::completion::CompletionItemKind,
    #[case] expected: LspCompletionItemKind,
) {
    let doc = lsp_textdocument::FullTextDocument::new(
        "json".to_string(),
        1,
        r#"{"name":"u"}"#.to_string(),
    );
    let item = crate::completion::DomainCompletion {
        label: "candidate".to_string(),
        kind,
        detail: None,
        insert_text: None,
        sort_priority: 7,
        replace_range_bytes: None,
    };
    let lowered = super::super::helpers::domain_to_lsp(item, &doc);
    assert_eq!(lowered.kind, Some(expected));
    assert_eq!(lowered.sort_text.as_deref(), Some("007candidate"));
}

#[test]
fn diagnostic_counts_and_position_helpers_cover_scalar_paths() {
    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 1,
        },
    };
    let diagnostics = vec![
        Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: None,
            message: "error".to_string(),
            related_information: None,
            tags: None,
            data: None,
        },
        Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::WARNING),
            code: None,
            code_description: None,
            source: None,
            message: "warning".to_string(),
            related_information: None,
            tags: None,
            data: None,
        },
        Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::INFORMATION),
            code: None,
            code_description: None,
            source: None,
            message: "info".to_string(),
            related_information: None,
            tags: None,
            data: None,
        },
    ];
    let counts = super::super::helpers::diagnostic_severity_counts(&diagnostics);
    assert_eq!(counts.errors, 1);
    assert_eq!(counts.warnings, 1);

    let source = "é\nid";
    let doc = lsp_textdocument::FullTextDocument::new("json".to_string(), 1, source.to_string());
    let pos = super::super::helpers::byte_to_ls_position(&doc, source.find("id").unwrap());
    assert_eq!(pos.line, 1);
    assert_eq!(pos.character, 0);
}

#[test]
fn domain_to_lsp_text_edit_clears_insert_text() {
    let source = r#"{"type":"i"}"#;
    let doc = lsp_textdocument::FullTextDocument::new("json".to_string(), 1, source.to_string());
    let replace_start = source.find('i').unwrap();
    let item = crate::completion::DomainCompletion {
        label: "integer".to_string(),
        kind: crate::completion::CompletionItemKind::Snippet,
        detail: Some("Integer".to_string()),
        insert_text: Some(r#"{"kind":"integer"}"#.to_string()),
        sort_priority: 2,
        replace_range_bytes: Some(replace_start..replace_start + 1),
    };

    let lowered = super::super::helpers::domain_to_lsp(item, &doc);
    assert!(lowered.insert_text.is_none());
    assert!(lowered.insert_text_format.is_some());
    let Some(CompletionTextEdit::Edit(edit)) = lowered.text_edit else {
        panic!("replace_range_bytes should lower to a text edit");
    };
    assert_eq!(edit.new_text, r#"{"kind":"integer"}"#);
}

#[test]
fn normalize_path_uses_existing_file_canonical_path() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("Model.JSON");
    std::fs::write(&path, "{}").unwrap();

    assert_eq!(
        super::super::helpers::normalize_path(&path),
        std::fs::canonicalize(&path).unwrap()
    );
}

#[rstest::rstest]
#[case::yaml("user.yaml", "name: user\ncolumns:\n  - name: email\n    type: text\n")]
#[case::json(
    "user.json",
    r#"{"name":"user","columns":[{"name":"email","type":"text"}]}"#
)]
fn disk_symbol_reference_and_rename_helpers_lower_disk_paths(
    #[case] file_name: &str,
    #[case] source: &str,
) {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join(file_name);
    std::fs::write(&path, source).unwrap();
    let target_uri = super::super::Backend::path_to_uri(&path).unwrap();
    let (service, _socket) = make_service();
    let backend = service.inner();
    let email_start = source.find("email").unwrap();
    let symbol = crate::symbols::DomainSymbol {
        name: "email".to_string(),
        kind: crate::symbols::SymbolKind::Column,
        container: Some("user".to_string()),
        uri: target_uri.clone(),
        byte_range: email_start..email_start + "email".len(),
    };
    let lowered_symbol = super::super::helpers::symbol_to_lsp(&symbol, backend)
        .expect("disk YAML symbol should lower");
    assert_eq!(lowered_symbol.kind, LspSymbolKind::FIELD);
    assert_eq!(lowered_symbol.location.uri, target_uri);
    let domain_edit = crate::rename::DomainTextEdit {
        byte_range: email_start..email_start + "email".len(),
        new_text: "mail".to_string(),
    };
    let edits = super::super::helpers::domain_edits_to_lsp(&target_uri, &[domain_edit], backend)
        .expect("disk YAML edits should lower");
    assert_eq!(edits[0].new_text, "mail");
    let reference = crate::references::DomainReference {
        uri: target_uri.clone(),
        byte_range: email_start..email_start + "email".len(),
    };
    let location = super::super::helpers::domain_reference_to_location(&reference, backend)
        .expect("disk YAML reference should lower");
    assert_eq!(location.uri, target_uri);
    assert!(location.range.end.character > location.range.start.character);
}

#[test]
fn disk_helper_returns_none_for_unreadable_reference_uri() {
    let (service, _socket) = make_service();
    let backend = service.inner();
    let missing = crate::references::DomainReference {
        uri: uri("file:///workspace/missing.yaml"),
        byte_range: 0..1,
    };
    assert!(super::super::helpers::domain_reference_to_location(&missing, backend).is_none());

    let missing_symbol = crate::symbols::DomainSymbol {
        name: "ghost".to_string(),
        kind: crate::symbols::SymbolKind::Table,
        container: None,
        uri: uri("file:///workspace/missing.json"),
        byte_range: 0..1,
    };
    assert!(super::super::helpers::symbol_to_lsp(&missing_symbol, backend).is_none());

    let missing_edit = crate::rename::DomainTextEdit {
        byte_range: 0..1,
        new_text: "ghost".to_string(),
    };
    assert!(
        super::super::helpers::domain_edits_to_lsp(
            &uri("file:///workspace/missing.json"),
            &[missing_edit],
            backend,
        )
        .is_none()
    );
}
