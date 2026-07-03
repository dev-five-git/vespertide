//! Drift detection — KILLER FEATURE.
//!
//! Compares current model files against the schema reconstructed from applied
//! migrations. Surfaces drift as workspace-level diagnostics so users can
//! generate a migration before forgetting.
//!
//! No live DB connection required — pure file-based comparison. This is the
//! feature no competitor (Prisma/sqls/postgres-lsp) provides.

mod actions;
mod cache;
mod compute;
mod sources;
mod types;

pub use cache::DriftCache;
pub use compute::{compute, compute_with_cache};
pub use types::{DomainDrift, DriftKind};

const _: fn(&vespertide_planner::PlannerError) -> Option<crate::diagnostics::ErrorLocation> =
    crate::diagnostics::ErrorLocation::from_planner_error;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use tempfile::tempdir;
    use tower_lsp_server::ls_types::Uri;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, SimpleColumnType, TableConstraint, TableDef,
    };

    use super::actions::{
        action_to_drift, lookup_baseline_column, render_column_type, render_comment,
        render_default, render_nullable,
    };
    use super::{DomainDrift, DriftKind, compute};
    use crate::store::DocumentStore;
    use crate::test_support::parse_json;
    use crate::workspace_index::WorkspaceIndex;

    fn column(name: &str, r#type: ColumnType, nullable: bool) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type,
            nullable,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table(name: &str, columns: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns,
            constraints: vec![],
        }
    }

    fn text_column(name: &str) -> ColumnDef {
        column(name, ColumnType::Simple(SimpleColumnType::Text), false)
    }

    fn integer_column(name: &str) -> ColumnDef {
        column(name, ColumnType::Simple(SimpleColumnType::Integer), false)
    }

    fn index_constraint(name: &str) -> TableConstraint {
        TableConstraint::Index {
            name: Some(name.into()),
            columns: vec!["email".into()],
        }
    }

    fn uri() -> Uri {
        Uri::from_str("file:///user.json").unwrap()
    }

    #[test]
    fn no_config_returns_empty() {
        let tmp = tempdir().unwrap();
        let idx = WorkspaceIndex::new();
        let docs = DocumentStore::new();
        let drifts = compute(tmp.path(), &idx, &docs);

        assert!(drifts.is_empty());
    }

    #[test]
    fn kind_codes_are_stable() {
        use DriftKind::*;
        assert_eq!(CreateTable.code(), "drift-create-table");
        assert_eq!(DeleteTable.code(), "drift-delete-table");
        assert_eq!(
            RenameTable {
                from: "a".into(),
                to: "b".into()
            }
            .code(),
            "drift-rename-table"
        );
        assert_eq!(AddColumn { column: "x".into() }.code(), "drift-add-column");
        assert_eq!(
            DeleteColumn { column: "x".into() }.code(),
            "drift-delete-column"
        );
        assert_eq!(
            RenameColumn {
                from: "a".into(),
                to: "b".into()
            }
            .code(),
            "drift-rename-column"
        );
        assert_eq!(
            ModifyColumnType {
                column: "x".into(),
                before: "integer".into(),
                after: "big_int".into()
            }
            .code(),
            "drift-modify-type"
        );
        assert_eq!(
            ModifyColumnNullable {
                column: "x".into(),
                before: true,
                after: false
            }
            .code(),
            "drift-modify-nullable"
        );
        assert_eq!(
            ModifyColumnDefault {
                column: "x".into(),
                before: None,
                after: Some("0".into())
            }
            .code(),
            "drift-modify-default"
        );
        assert_eq!(
            ModifyColumnComment {
                column: "x".into(),
                before: None,
                after: Some("c".into())
            }
            .code(),
            "drift-modify-comment"
        );
        assert_eq!(AddConstraint { name: None }.code(), "drift-add-constraint");
        assert_eq!(
            RemoveConstraint { name: None }.code(),
            "drift-remove-constraint"
        );
        assert_eq!(
            ReplaceConstraint { name: None }.code(),
            "drift-replace-constraint"
        );
        assert_eq!(RawSql.code(), "drift-raw-sql");
    }

    #[test]
    fn baseline_lookup_finds_column() {
        let baseline = vec![table("user", vec![integer_column("id")])];

        let found = lookup_baseline_column(&baseline, "user", "id");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "id");
        assert!(lookup_baseline_column(&baseline, "user", "email").is_none());
        assert!(lookup_baseline_column(&baseline, "post", "id").is_none());
    }

    #[test]
    fn render_helpers_format_correctly() {
        let int_type = ColumnType::Simple(SimpleColumnType::Integer);
        assert!(render_column_type(&int_type).contains("Integer"));
        assert_eq!(render_default(None), "<none>");
        assert_eq!(render_default(Some("0")), "\"0\"");
        assert_eq!(render_nullable(true), "nullable");
        assert_eq!(render_nullable(false), "not null");
        assert_eq!(render_comment(None), "<none>");
        assert_eq!(render_comment(Some("user id")), "\"user id\"");
    }

    #[test]
    fn action_to_drift_create_table() {
        let action = MigrationAction::CreateTable {
            table: "user".into(),
            columns: vec![],
            constraints: vec![],
        };
        let source = r#"{"name":"user","columns":[]}"#;
        let tree = parse_json(source);

        let (kind, byte_range, message) =
            action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(kind, DriftKind::CreateTable);
        assert!(byte_range.is_some());
        assert!(message.contains("Table 'user'"));
    }

    #[test]
    fn action_to_drift_add_column() {
        let action = MigrationAction::AddColumn {
            table: "user".into(),
            column: Box::new(text_column("email")),
            fill_with: None,
        };
        let source =
            r#"{"name":"user","columns":[{"name":"email","type":"text","nullable":false}]}"#;
        let tree = parse_json(source);

        let (kind, byte_range, message) =
            action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(
            kind,
            DriftKind::AddColumn {
                column: "email".into()
            }
        );
        assert!(byte_range.is_some());
        assert!(message.contains("email"));
    }

    #[test]
    fn action_to_drift_modify_column_type() {
        let baseline = vec![table("user", vec![integer_column("id")])];
        let action = MigrationAction::ModifyColumnType {
            table: "user".into(),
            column: "id".into(),
            new_type: ColumnType::Simple(SimpleColumnType::BigInt),
            fill_with: None,
            narrowing_strategy: None,
            timezone: None,
        };
        let source =
            r#"{"name":"user","columns":[{"name":"id","type":"big_int","nullable":false}]}"#;
        let tree = parse_json(source);

        let (_, _, message) = action_to_drift(&action, &baseline, source, Some(&tree)).unwrap();

        assert!(message.contains("Integer"));
        assert!(message.contains("BigInt"));
    }

    #[test]
    fn action_to_drift_modify_column_nullable_false_to_true() {
        let baseline = vec![table("user", vec![text_column("email")])];
        let action = MigrationAction::ModifyColumnNullable {
            table: "user".into(),
            column: "email".into(),
            nullable: true,
            fill_with: None,
            delete_null_rows: None,
        };
        let source =
            r#"{"name":"user","columns":[{"name":"email","type":"text","nullable":true}]}"#;
        let tree = parse_json(source);

        let (_, _, message) = action_to_drift(&action, &baseline, source, Some(&tree)).unwrap();

        assert!(message.contains("not null"));
        assert!(message.contains("nullable"));
    }

    #[test]
    fn action_to_drift_modify_column_default_none_to_some() {
        let baseline = vec![table("user", vec![integer_column("id")])];
        let action = MigrationAction::ModifyColumnDefault {
            table: "user".into(),
            column: "id".into(),
            new_default: Some("0".into()),
            backfill: None,
        };
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"default":"0"}]}"#;
        let tree = parse_json(source);

        let (_, _, message) = action_to_drift(&action, &baseline, source, Some(&tree)).unwrap();

        assert!(message.contains("<none>"));
        assert!(message.contains("\"0\""));
    }

    #[test]
    fn action_to_drift_modify_column_comment_none_to_some() {
        let baseline = vec![table("user", vec![integer_column("id")])];
        let action = MigrationAction::ModifyColumnComment {
            table: "user".into(),
            column: "id".into(),
            new_comment: Some("user id".into()),
        };
        let source = r#"{"name":"user","columns":[{"name":"id","type":"integer","nullable":false,"comment":"user id"}]}"#;
        let tree = parse_json(source);

        let (_, _, message) = action_to_drift(&action, &baseline, source, Some(&tree)).unwrap();

        assert!(message.contains("<none>"));
        assert!(message.contains("user id"));
    }

    #[test]
    fn action_to_drift_delete_column() {
        let baseline = vec![table("user", vec![text_column("email")])];
        let action = MigrationAction::DeleteColumn {
            table: "user".into(),
            column: "email".into(),
        };
        let source = r#"{"name":"user","columns":[]}"#;
        let tree = parse_json(source);

        let (kind, byte_range, message) =
            action_to_drift(&action, &baseline, source, Some(&tree)).unwrap();

        assert_eq!(
            kind,
            DriftKind::DeleteColumn {
                column: "email".into()
            }
        );
        assert!(byte_range.is_some());
        assert!(message.contains("email"));
    }

    #[test]
    fn action_to_drift_rename_column() {
        let action = MigrationAction::RenameColumn {
            table: "user".into(),
            from: "old".into(),
            to: "new_name".into(),
        };
        let source =
            r#"{"name":"user","columns":[{"name":"new_name","type":"text","nullable":false}]}"#;
        let tree = parse_json(source);

        let (kind, _, message) = action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(
            kind,
            DriftKind::RenameColumn {
                from: "old".into(),
                to: "new_name".into()
            }
        );
        assert!(message.contains("old"));
        assert!(message.contains("new_name"));
    }

    #[test]
    fn action_to_drift_create_table_top_name_position() {
        let action = MigrationAction::CreateTable {
            table: "user".into(),
            columns: vec![],
            constraints: vec![],
        };
        let source = r#"{"name":"user","columns":[]}"#;
        let tree = parse_json(source);

        let (_, byte_range, _) = action_to_drift(&action, &[], source, Some(&tree)).unwrap();
        let range = byte_range.unwrap();

        assert_eq!(&source[range], "\"user\"");
    }

    #[test]
    fn action_to_drift_rename_table() {
        let action = MigrationAction::RenameTable {
            from: "users".into(),
            to: "user".into(),
        };
        let source = r#"{"name":"user","columns":[]}"#;
        let tree = parse_json(source);

        let (kind, _, message) = action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(
            kind,
            DriftKind::RenameTable {
                from: "users".into(),
                to: "user".into()
            }
        );
        assert!(message.contains("users"));
        assert!(message.contains("user"));
    }

    #[test]
    fn action_to_drift_delete_table() {
        let action = MigrationAction::DeleteTable {
            table: "ghost".into(),
        };
        let source = r#"{"columns":[]}"#;
        let tree = parse_json(source);

        let (kind, byte_range, message) =
            action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(kind, DriftKind::DeleteTable);
        assert!(byte_range.is_some());
        assert!(message.contains("ghost"));
    }

    #[test]
    fn action_to_drift_add_constraint() {
        let constraint = index_constraint("ix_user__email");
        let action = MigrationAction::AddConstraint {
            table: "user".into(),
            constraint,
        };
        let source = r#"{"name":"user","columns":[],"constraints":[{"type":"index","name":"ix_user__email","columns":["email"]}]}"#;
        let tree = parse_json(source);

        let (kind, byte_range, message) =
            action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(
            kind,
            DriftKind::AddConstraint {
                name: Some("ix_user__email".into())
            }
        );
        assert!(byte_range.is_some());
        assert!(message.contains("ix_user__email"));
    }

    #[test]
    fn action_to_drift_remove_constraint() {
        let constraint = index_constraint("ix_user__email");
        let action = MigrationAction::RemoveConstraint {
            table: "user".into(),
            constraint,
        };
        let source = r#"{"name":"user","columns":[]}"#;
        let tree = parse_json(source);

        let (kind, byte_range, message) =
            action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(
            kind,
            DriftKind::RemoveConstraint {
                name: Some("ix_user__email".into())
            }
        );
        assert!(byte_range.is_some());
        assert!(message.contains("ix_user__email"));
    }

    #[test]
    fn action_to_drift_replace_constraint() {
        let from = index_constraint("ix_user__email_old");
        let to = index_constraint("ix_user__email_new");
        let action = MigrationAction::ReplaceConstraint {
            table: "user".into(),
            from,
            to,
        };
        let source = r#"{"name":"user","columns":[],"constraints":[{"type":"index","name":"ix_user__email_new","columns":["email"]}]}"#;
        let tree = parse_json(source);

        let (kind, _, message) = action_to_drift(&action, &[], source, Some(&tree)).unwrap();

        assert_eq!(
            kind,
            DriftKind::ReplaceConstraint {
                name: Some("ix_user__email_new".into())
            }
        );
        assert!(message.contains("ix_user__email_old"));
        assert!(message.contains("ix_user__email_new"));
    }

    #[test]
    fn action_to_drift_raw_sql() {
        let action = MigrationAction::RawSql {
            sql: "SELECT 1".into(),
        };

        let (kind, byte_range, message) = action_to_drift(&action, &[], "", None).unwrap();

        assert_eq!(kind, DriftKind::RawSql);
        assert_eq!(byte_range, None);
        assert!(message.contains("Raw SQL"));
    }

    #[test]
    fn into_domain_diagnostic_drops_none_range() {
        let drift = DomainDrift {
            uri: uri(),
            kind: DriftKind::RawSql,
            byte_range: None,
            message: "m".into(),
        };

        assert!(drift.into_domain_diagnostic().is_none());
    }

    #[test]
    fn into_domain_diagnostic_preserves_range_and_code() {
        let drift = DomainDrift {
            uri: uri(),
            kind: DriftKind::AddColumn { column: "x".into() },
            byte_range: Some(5..10),
            message: "m".into(),
        };

        let diagnostic = drift.into_domain_diagnostic().unwrap();

        assert_eq!(diagnostic.byte_range, 5..10);
        assert_eq!(diagnostic.code, "drift-add-column");
        assert_eq!(
            diagnostic.severity,
            crate::diagnostics::Severity::Information
        );
        assert_eq!(diagnostic.message, "m");
    }
}
