//! Validation routines: syntax → parse → planner.

mod cache;
mod parse;
mod types;
mod visitors;

use tower_lsp_server::ls_types::Uri;
use vespertide_core::{MigrationAction, MigrationPlan, SimpleColumnType, TableDef};

use super::{DomainDiagnostic, Severity};

pub(super) use parse::{try_parse_json, try_parse_yaml};
pub(super) use visitors::collect_all;
// Per-collector entry points exist only as test oracles for
// `fused_walk_matches_unfused_pipeline` (see diagnostics/mod.rs). Production
// uses the fused `collect_all` path exclusively.
#[cfg(test)]
pub(super) use visitors::{
    collect_complex_type_violations, collect_duplicate_column_names, collect_syntax_errors,
    collect_unknown_column_types,
};

/// Parsed table plus source context for workspace-wide validation.
pub struct WorkspaceTable {
    /// URI that owns this table definition.
    pub uri: Uri,
    /// Normalized table definition used by planner validation.
    pub table: TableDef,
    /// Raw document text used for byte-range location.
    pub source: String,
    /// Parsed tree-sitter tree for source range lookup.
    pub tree: Option<tree_sitter::Tree>,
}

pub(super) fn validate_table(
    table: &TableDef,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    // Single-table validation. `vespertide_planner::find_schema_violations`
    // returns every violation in a single pass; we surface each as its own
    // diagnostic so the editor shows one squiggle per problem instead of
    // collapsing them into a single "validate-schema" entry. The locator is
    // run per-violation so each squiggle lands on the responsible column /
    // constraint inside this file.
    for violation in vespertide_planner::find_schema_violations(std::slice::from_ref(table)) {
        if is_dedicated_check_violation(&violation) {
            continue;
        }
        let byte_range = byte_range_for_violation(&violation, tree, source);
        out.push(DomainDiagnostic {
            byte_range,
            severity: Severity::Error,
            message: violation.to_string(),
            code: "validate-schema".to_string(),
        });
    }
}

/// Resolve the source byte range for a planner violation against this file.
///
/// Falls back through three layers:
/// 1. Locator returns column-level location → squiggle the responsible
///    column field (or column object).
/// 2. Locator returns constraint-level location → squiggle that constraint.
/// 3. Locator returns table-level (or `None`) → squiggle the top-level
///    `name` key, or `0..1` as a last resort.
fn byte_range_for_violation(
    err: &vespertide_planner::PlannerError,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
) -> std::ops::Range<usize> {
    let Some(loc) = super::locator::ErrorLocation::from_planner_error(err) else {
        return 0..1;
    };
    if let Some(column) = &loc.column {
        return if let Some(field) = loc.field {
            super::locator::locate_column_field(tree, source, column, field)
        } else {
            super::locator::locate_column(tree, source, column)
        };
    }
    if let Some(constraint) = &loc.constraint {
        return super::locator::locate_constraint(tree, source, constraint);
    }
    super::locator::locate_top_name(tree, source).unwrap_or(0..1)
}

/// Fault **F51**: foreign-key constraints whose referencing columns are not
/// covered by any leading-prefix index on the child table.
///
/// Emits one `Warning`-severity `DomainDiagnostic` per uncovered FK. The
/// range is anchored — in priority order — to:
/// 1. the inline `foreign_key` value on the first referencing column;
/// 2. a named table-level `constraints` entry, if `constraint_name` is set;
/// 3. `0..1` as a final fallback.
///
/// Static: this performs no data access; it only inspects the normalised
/// `TableDef`. The caller must pass the source text and tree corresponding to
/// the file that produced `table` so the squiggle lands on the responsible
/// region.
pub(super) fn validate_fk_supporting_indexes(
    table: &TableDef,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    for missing in
        vespertide_planner::find_missing_fk_supporting_indexes(std::slice::from_ref(table))
    {
        let byte_range = locate_missing_fk(&missing, tree, source);
        out.push(DomainDiagnostic {
            byte_range,
            severity: Severity::Warning,
            message: format!(
                "Foreign key on ({}) lacks a supporting index. \
                 Cascade and lookup operations will scan the entire `{}` table. \
                 Suggested index: `{}`.",
                missing.columns.join(", "),
                missing.table,
                missing.suggested_index_name,
            ),
            code: "fk-supporting-index".to_string(),
        });
    }
}

/// Fault **F76**: file-local sequence/identity exhaustion risk on
/// single-column auto-increment primary keys typed `INTEGER` (32-bit)
/// or `SMALLINT` (16-bit).
///
/// Emits one `Warning`-severity `DomainDiagnostic` per risky PK
/// column, anchored to the column's `type` field. The squiggle is
/// purely advisory — vespertide cannot rewrite the column to
/// `BigInt` from the editor; the user runs `vespertide revision`
/// which surfaces the same risk with an interactive
/// `ChangeToBigInt` choice.
///
/// **LSP scope**: only the `Primary` kind fires here. The planner
/// also detects `PkTypeNarrowing` (requires baseline migration
/// history) and `ForeignKeyMismatch` (requires the parent table's
/// `TableDef` from another file) — both are intentionally out of
/// scope for this single-file warning. They surface during
/// `vespertide revision`.
pub(super) fn validate_sequence_exhaustion(
    table: &TableDef,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    // Synthesise a single-action `CreateTable` plan against an empty
    // baseline. The planner's `Primary` arm fires for every new
    // single-column auto-increment INT4/SmallInt PK in `table`,
    // which is exactly the file-local risk we want to surface in
    // the editor. `PkTypeNarrowing` and `ForeignKeyMismatch` arms
    // are naturally suppressed: the former needs a non-empty
    // baseline; the latter needs the parent PK column type, which
    // an empty `baseline` does not provide.
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 0,
        actions: vec![MigrationAction::CreateTable {
            table: table.name.clone(),
            columns: table.columns.clone(),
            constraints: table.constraints.clone(),
        }],
    };
    for warning in vespertide_planner::find_sequence_exhaustion_risks(&plan, &[]) {
        let byte_range = super::locator::locate_column_field(
            tree,
            source,
            &warning.column,
            super::locator::ErrorField::Type,
        );
        let byte_range = if byte_range == (0..1) {
            super::locator::locate_column(tree, source, &warning.column)
        } else {
            byte_range
        };
        let risk_label = match warning.risk_level {
            vespertide_planner::SequenceRiskLevel::High => {
                "High — `small_int` exhausts at ~32K rows"
            }
            vespertide_planner::SequenceRiskLevel::Medium => {
                "Medium — `integer` exhausts at ~2.1B rows"
            }
        };
        out.push(DomainDiagnostic {
            byte_range,
            severity: Severity::Warning,
            message: format!(
                "Auto-increment primary key `{}` uses `{}` — sequence exhaustion risk. \
                 {}. Consider rewriting to `big_int` (run `vespertide revision` for an \
                 interactive prompt).",
                warning.column,
                simple_type_label(warning.current_type),
                risk_label,
            ),
            code: "sequence-exhaustion".to_string(),
        });
    }
}

/// Fault **F-novel-4**: CHECK constraint literal type-mismatch detection.
///
/// Emits one `Warning`-severity `DomainDiagnostic` per
/// `CheckTypeMismatchWarning` produced by the planner. Synthesises a
/// single-action `CreateTable` plan from the parsed `TableDef` and
/// queries `find_check_type_mismatches` with an empty baseline to
/// surface file-local warnings. The squiggle is anchored to the
/// `constraints` entry by name; column-level fallback when the
/// constraint cannot be located.
pub(super) fn validate_check_type_mismatches(
    table: &TableDef,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    let plan = MigrationPlan {
        id: String::new(),
        comment: None,
        created_at: None,
        version: 0,
        actions: vec![MigrationAction::CreateTable {
            table: table.name.clone(),
            columns: table.columns.clone(),
            constraints: table.constraints.clone(),
        }],
    };
    for warning in vespertide_planner::find_check_type_mismatches(&plan, &[]) {
        let constraint_range =
            super::locator::locate_constraint(tree, source, &warning.constraint_name);
        let byte_range = if constraint_range == (0..1) {
            super::locator::locate_column(tree, source, &warning.column)
        } else {
            constraint_range
        };
        out.push(DomainDiagnostic {
            byte_range,
            severity: Severity::Warning,
            message: format!(
                "CHECK constraint `{}` on `{}.{}` compares a `{}` column to a {} literal `{}` \
                 — this is a likely type error. Backend-specific coercion may silently succeed \
                 on MySQL/SQLite but PostgreSQL rejects it at ADD CONSTRAINT time.",
                warning.constraint_name,
                warning.table,
                warning.column,
                warning.column_type_label,
                warning.literal_kind.to_lowercase(),
                warning.literal_text,
            ),
            code: "check-type-mismatch".to_string(),
        });
    }
}

/// Fault **F-novel-15** (BETWEEN boundary reversed) — collect-all,
/// constraint-anchored. Replaces the first-fail `validate_schema` path for this
/// fault in the LSP.
pub(super) fn validate_check_between_order(
    table: &TableDef,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    for err in vespertide_planner::find_between_boundary_reversals(table) {
        if let vespertide_planner::PlannerError::BetweenBoundaryReversed { check_name, .. } = &err {
            let byte_range = super::locator::locate_constraint(tree, source, check_name);
            out.push(DomainDiagnostic {
                byte_range,
                severity: Severity::Error,
                message: err.to_string(),
                code: "check-between-reversed".to_string(),
            });
        }
    }
}

/// Fault **F-novel-1** (CHECK self-contradiction) — collect-all,
/// constraint-anchored. Replaces the first-fail `validate_schema` path for this
/// fault in the LSP.
pub(super) fn validate_check_self_contradiction(
    table: &TableDef,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    for err in vespertide_planner::find_self_contradictions(table) {
        if let vespertide_planner::PlannerError::CheckSelfContradiction { check_name, .. } = &err {
            let byte_range = super::locator::locate_constraint(tree, source, check_name);
            out.push(DomainDiagnostic {
                byte_range,
                severity: Severity::Error,
                message: err.to_string(),
                code: "check-self-contradiction".to_string(),
            });
        }
    }
}

fn is_dedicated_check_violation(err: &vespertide_planner::PlannerError) -> bool {
    matches!(
        err,
        vespertide_planner::PlannerError::BetweenBoundaryReversed { .. }
            | vespertide_planner::PlannerError::CheckSelfContradiction { .. }
    )
}

fn simple_type_label(ty: SimpleColumnType) -> &'static str {
    match ty {
        SimpleColumnType::SmallInt => "small_int",
        SimpleColumnType::Integer => "integer",
        SimpleColumnType::BigInt => "big_int",
        _ => "<other>",
    }
}

fn locate_missing_fk(
    missing: &vespertide_planner::MissingFkSupportingIndex,
    tree: Option<&tree_sitter::Tree>,
    source: &str,
) -> std::ops::Range<usize> {
    // Prefer the inline `foreign_key` value on the first referencing column;
    // this is the canonical Vespertide authoring style and gives the most
    // precise squiggle. `locate_column_field` already falls back to the
    // column object range if the inline FK pair is absent.
    if let Some(first) = missing.columns.first() {
        let range = super::locator::locate_column_field(
            tree,
            source,
            first,
            super::locator::ErrorField::ForeignKeyRefTable,
        );
        if range != (0..1) {
            return range;
        }
    }
    if let Some(name) = &missing.constraint_name {
        return super::locator::locate_constraint(tree, source, name);
    }
    0..1
}

/// Compare the file's basename to its declared table `name` and surface a
/// warning when they diverge. This catches accidental renames where the
/// user changes `"name"` but forgets to rename the file (or vice versa).
///
/// Path → basename rules (longest extension wins):
///   `foo.vespertide.json` → `foo`
///   `foo.vespertide.yaml` → `foo`
///   `foo.vespertide.yml`  → `foo`
///   `foo.json` / `foo.yaml` / `foo.yml` → `foo`
pub(super) fn check_filename_table_name_mismatch(
    text: &str,
    uri: &Uri,
    tree: Option<&tree_sitter::Tree>,
    table_name: &str,
    out: &mut Vec<DomainDiagnostic>,
) {
    if let Some(file_basename) = file_basename_of(uri)
        && file_basename != table_name
    {
        let byte_range = super::locator::locate_top_name(tree, text).unwrap_or(0..1);
        out.push(DomainDiagnostic {
            byte_range,
            severity: Severity::Warning,
            message: format!(
                "Table name `{table_name}` does not match file basename `{file_basename}`. \
                 Rename one to keep them in sync."
            ),
            code: "filename-mismatch".to_string(),
        });
    }
}

fn file_basename_of(uri: &Uri) -> Option<String> {
    let path = crate::position::uri_to_path(uri)?;
    let file_name = path.file_name()?.to_str()?;
    let stripped = file_name
        .strip_suffix(".vespertide.json")
        .or_else(|| file_name.strip_suffix(".vespertide.yaml"))
        .or_else(|| file_name.strip_suffix(".vespertide.yml"))
        .or_else(|| file_name.strip_suffix(".json"))
        .or_else(|| file_name.strip_suffix(".yaml"))
        .or_else(|| file_name.strip_suffix(".yml"))
        .unwrap_or(file_name);
    Some(stripped.to_string())
}

pub(super) fn validate_workspace(
    workspace: &[WorkspaceTable],
    current_uri: &Uri,
    out: &mut Vec<DomainDiagnostic>,
) {
    let tables: Vec<TableDef> = workspace.iter().map(|entry| entry.table.clone()).collect();
    let violations = vespertide_planner::find_schema_violations(&tables);
    if violations.is_empty() {
        return;
    }

    // Per-violation publish: each problem becomes one diagnostic, anchored
    // to whichever workspace file owns the offending table. Diagnostics for
    // other files are dropped here — they will be surfaced when the editor
    // requests diagnostics for *those* files (LSP protocol is per-URI).
    for err in &violations {
        if is_dedicated_check_violation(err) {
            continue;
        }
        let Some(location) = super::locator::ErrorLocation::from_planner_error(err) else {
            // Cross-cutting error (no table anchor) — attach to current file
            // as a generic top-of-document squiggle so it is visible.
            push_validate_error(out, 0..1, err.to_string());
            continue;
        };

        emit_workspace_location_violation(workspace, current_uri, out, err.to_string(), &location);
    }
}

fn emit_workspace_location_violation(
    workspace: &[WorkspaceTable],
    current_uri: &Uri,
    out: &mut Vec<DomainDiagnostic>,
    message: String,
    location: &super::locator::ErrorLocation,
) {
    let Some(target) = workspace
        .iter()
        .find(|entry| entry.table.name.as_str() == location.table.as_str())
    else {
        push_validate_error(out, 0..1, message);
        return;
    };

    if target.uri != *current_uri {
        return;
    }

    let byte_range = if let Some(column) = &location.column {
        if let Some(field) = location.field {
            super::locator::locate_column_field(target.tree.as_ref(), &target.source, column, field)
        } else {
            super::locator::locate_column(target.tree.as_ref(), &target.source, column)
        }
    } else if let Some(constraint) = &location.constraint {
        super::locator::locate_constraint(target.tree.as_ref(), &target.source, constraint)
    } else {
        super::locator::locate_top_name(target.tree.as_ref(), &target.source).unwrap_or(0..1)
    };

    push_validate_error(out, byte_range, message);
}

fn push_validate_error(
    out: &mut Vec<DomainDiagnostic>,
    byte_range: std::ops::Range<usize>,
    message: String,
) {
    out.push(DomainDiagnostic {
        byte_range,
        severity: Severity::Error,
        message,
        code: "validate-schema".to_string(),
    });
}

fn byte_offset_for_line_col(text: &str, line: usize, col: usize) -> usize {
    // serde_json line/column values are 1-indexed.
    let line_zero = line.saturating_sub(1);
    let col_zero = col.saturating_sub(1);
    let mut byte = 0;

    for (idx, line_text) in text.split_inclusive('\n').enumerate() {
        if idx == line_zero {
            return byte + col_zero.min(line_text.len().saturating_sub(1));
        }
        byte += line_text.len();
    }

    byte.min(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use crate::test_support::uri;
    use tower_lsp_server::ls_types::Uri;
    use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint};
    use vespertide_planner::{MissingFkSupportingIndex, PlannerError};

    fn col(name: &str) -> ColumnDef {
        ColumnDef::new(name, ColumnType::Simple(SimpleColumnType::Integer), false)
    }

    fn table_with_constraints(name: &str, constraints: Vec<TableConstraint>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns: vec![col("id")],
            constraints,
        }
    }

    #[test]
    fn byte_range_for_table_validation_uses_default_range() {
        let err = PlannerError::TableValidation("not locatable".to_string());

        assert_eq!(byte_range_for_violation(&err, None, ""), 0..1);
    }

    #[test]
    fn simple_type_label_covers_bigint_and_fallback() {
        assert_eq!(simple_type_label(SimpleColumnType::BigInt), "big_int");
        assert_eq!(simple_type_label(SimpleColumnType::Text), "<other>");
    }

    #[test]
    fn locate_missing_fk_falls_back_to_named_constraint_then_default() {
        let missing_named = MissingFkSupportingIndex {
            table: "posts".into(),
            constraint_name: Some("fk_posts_user".into()),
            columns: Vec::new(),
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
            suggested_index_name: "ix_posts__user_id".into(),
        };
        let missing_unnamed = MissingFkSupportingIndex {
            constraint_name: None,
            ..missing_named.clone()
        };

        assert_eq!(locate_missing_fk(&missing_named, None, ""), 0..1);
        assert_eq!(locate_missing_fk(&missing_unnamed, None, ""), 0..1);
    }

    #[test]
    fn filename_mismatch_ignores_non_file_uri() {
        let mut out = Vec::new();
        let uri = Uri::from_str("https://example.com/user.json").unwrap();

        check_filename_table_name_mismatch(r#"{"name":"user"}"#, &uri, None, "user", &mut out);

        assert!(out.is_empty());
    }

    #[test]
    fn validate_workspace_surfaces_cross_cutting_table_validation_on_current_file() {
        let current_uri = uri("dup.json");
        let table = TableDef {
            name: "dup".into(),
            description: None,
            columns: vec![col("id"), col("id")],
            constraints: Vec::new(),
        };
        let workspace = vec![WorkspaceTable {
            uri: current_uri.clone(),
            table,
            source: r#"{"name":"dup"}"#.to_string(),
            tree: None,
        }];
        let mut out = Vec::new();

        validate_workspace(&workspace, &current_uri, &mut out);

        assert!(
            out.iter()
                .any(|diag| diag.byte_range == (0..1) && diag.code == "validate-schema"),
            "got: {out:?}"
        );
    }

    #[test]
    fn validate_workspace_anchors_constraint_level_violations() {
        let current_uri = uri("users.json");
        let source = r#"{"name":"users","columns":[{"name":"id","type":"integer"}],"constraints":[{"type":"index","name":"ix_empty","columns":[]}]}"#;
        let tree = crate::parser::ParserPool::new()
            .parse(source, crate::parser::DocumentFormat::Json)
            .unwrap();
        let table = table_with_constraints(
            "users",
            vec![
                TableConstraint::PrimaryKey {
                    columns: vec!["id".into()],
                    auto_increment: false,
                    strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
                },
                TableConstraint::Index {
                    name: Some("ix_empty".into()),
                    columns: Vec::new(),
                },
            ],
        );
        let workspace = vec![WorkspaceTable {
            uri: current_uri.clone(),
            table,
            source: source.to_string(),
            tree: Some(tree),
        }];
        let mut out = Vec::new();

        validate_workspace(&workspace, &current_uri, &mut out);

        assert!(
            out.iter()
                .any(|diag| diag.code == "validate-schema" && diag.message.contains("ix_empty")),
            "got: {out:?}"
        );
    }

    #[test]
    fn emit_workspace_location_violation_handles_missing_and_other_file_targets() {
        let current_uri = uri("current.json");
        let other_uri = uri("other.json");
        let table = TableDef {
            name: "other".into(),
            description: None,
            columns: vec![col("id")],
            constraints: Vec::new(),
        };
        let workspace = vec![WorkspaceTable {
            uri: other_uri,
            table,
            source: r#"{"name":"other"}"#.to_string(),
            tree: None,
        }];
        let missing = super::super::locator::ErrorLocation {
            table: "missing".to_string(),
            column: None,
            constraint: None,
            field: None,
        };
        let other = super::super::locator::ErrorLocation {
            table: "other".to_string(),
            column: None,
            constraint: None,
            field: None,
        };
        let mut out = Vec::new();

        emit_workspace_location_violation(
            &workspace,
            &current_uri,
            &mut out,
            "missing target".to_string(),
            &missing,
        );
        emit_workspace_location_violation(
            &workspace,
            &current_uri,
            &mut out,
            "other target".to_string(),
            &other,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].byte_range, 0..1);
        assert_eq!(out[0].message, "missing target");
    }

    #[test]
    fn byte_offset_for_line_col_walks_past_prior_lines() {
        assert_eq!(
            byte_offset_for_line_col("one\ntwo\nthree", 3, 2),
            "one\ntwo\nt".len()
        );
    }
}
