//! Map `DomainDiagnostic` → `tower_lsp_server::ls_types::Diagnostic`.

use lsp_textdocument::FullTextDocument;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use super::{DomainDiagnostic, Severity};
use crate::position::byte_to_lsp_position;

#[must_use]
pub fn to_lsp(domain: &DomainDiagnostic, doc: &FullTextDocument) -> Diagnostic {
    let start = byte_to_lsp_position(doc, domain.byte_range.start);
    let end = byte_to_lsp_position(doc, domain.byte_range.end);

    Diagnostic {
        range: tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position {
                line: start.line,
                character: start.character,
            },
            end: tower_lsp_server::ls_types::Position {
                line: end.line,
                character: end.character,
            },
        },
        severity: Some(match domain.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Information => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(NumberOrString::String(domain.code.clone())),
        code_description: None,
        source: Some("vespertide-lsp".to_string()),
        message: domain.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::error(Severity::Error, DiagnosticSeverity::ERROR)]
    #[case::warning(Severity::Warning, DiagnosticSeverity::WARNING)]
    #[case::information(Severity::Information, DiagnosticSeverity::INFORMATION)]
    #[case::hint(Severity::Hint, DiagnosticSeverity::HINT)]
    fn to_lsp_maps_severity_variants(
        #[case] severity: Severity,
        #[case] expected: DiagnosticSeverity,
    ) {
        let doc = FullTextDocument::new("json".to_string(), 1, "hello\nworld".to_string());
        let domain = DomainDiagnostic {
            byte_range: 0..5,
            severity,
            message: "msg".to_string(),
            code: "test-code".to_string(),
        };

        let lsp = to_lsp(&domain, &doc);

        assert_eq!(lsp.severity, Some(expected));
        assert_eq!(lsp.message, "msg");
        assert_eq!(
            lsp.source,
            Some("vespertide-lsp".to_string()),
            "source must be set"
        );
        assert!(lsp.code.is_some());
    }
}
