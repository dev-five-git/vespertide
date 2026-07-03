//! Drift kind enumeration and the `DomainDrift` record.
//!
//! These are the public-facing data shapes for the drift API. Internal
//! `DriftRecord` is the tuple shape returned by the per-action dispatcher in
//! [`super::actions`] before it is folded into a `DomainDrift`.

use std::ops::Range;

use tower_lsp_server::ls_types::Uri;

pub(super) type DriftRecord = (DriftKind, Option<Range<usize>>, String);

/// Categorizes a single migration action for drift diagnostics.
///
/// Each variant corresponds to a specific type of schema change. The `code()` method
/// returns a stable diagnostic code suitable for LSP clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    CreateTable,
    DeleteTable,
    RenameTable {
        from: String,
        to: String,
    },
    AddColumn {
        column: String,
    },
    DeleteColumn {
        column: String,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    ModifyColumnType {
        column: String,
        before: String,
        after: String,
    },
    ModifyColumnNullable {
        column: String,
        before: bool,
        after: bool,
    },
    ModifyColumnDefault {
        column: String,
        before: Option<String>,
        after: Option<String>,
    },
    ModifyColumnComment {
        column: String,
        before: Option<String>,
        after: Option<String>,
    },
    AddConstraint {
        name: Option<String>,
    },
    RemoveConstraint {
        name: Option<String>,
    },
    ReplaceConstraint {
        name: Option<String>,
    },
    RawSql,
}

impl DriftKind {
    /// Returns a stable diagnostic code for this drift kind.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::CreateTable => "drift-create-table",
            Self::DeleteTable => "drift-delete-table",
            Self::RenameTable { .. } => "drift-rename-table",
            Self::AddColumn { .. } => "drift-add-column",
            Self::DeleteColumn { .. } => "drift-delete-column",
            Self::RenameColumn { .. } => "drift-rename-column",
            Self::ModifyColumnType { .. } => "drift-modify-type",
            Self::ModifyColumnNullable { .. } => "drift-modify-nullable",
            Self::ModifyColumnDefault { .. } => "drift-modify-default",
            Self::ModifyColumnComment { .. } => "drift-modify-comment",
            Self::AddConstraint { .. } => "drift-add-constraint",
            Self::RemoveConstraint { .. } => "drift-remove-constraint",
            Self::ReplaceConstraint { .. } => "drift-replace-constraint",
            Self::RawSql => "drift-raw-sql",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDrift {
    /// URI of the model file with drift.
    pub uri: Uri,
    /// Specific drift category for diagnostic codes and downstream routing.
    pub kind: DriftKind,
    /// Source byte range to anchor the diagnostic, when one is available.
    pub byte_range: Option<Range<usize>>,
    /// User-facing drift message.
    pub message: String,
}

impl DomainDrift {
    /// Convert into a `DomainDiagnostic`. Returns `None` when `byte_range`
    /// is `None` — those drifts have no anchorable position and are
    /// dropped silently (matches the current behaviour of skipping unknown
    /// positions).
    #[must_use]
    pub fn into_domain_diagnostic(self) -> Option<crate::diagnostics::DomainDiagnostic> {
        let range = self.byte_range?;
        Some(crate::diagnostics::DomainDiagnostic {
            byte_range: range,
            severity: crate::diagnostics::Severity::Information,
            message: self.message,
            code: self.kind.code().to_string(),
        })
    }
}
