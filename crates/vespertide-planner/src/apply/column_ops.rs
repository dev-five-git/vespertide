use std::collections::BTreeMap;

use vespertide_core::{
    ColumnDef, ColumnName, ColumnType, ComplexColumnType, EnumValues, TableConstraint, TableDef,
};

use crate::error::PlannerError;

pub(super) fn add_column(
    schema: &mut [TableDef],
    table: &str,
    column: &ColumnDef,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    if tbl.columns.iter().any(|c| c.name == column.name) {
        Err(PlannerError::ColumnExists(
            table.to_string(),
            column.name.to_string(),
        ))
    } else {
        tbl.columns.push(column.clone());
        // Re-normalize to promote any inline constraints on the new column
        // to table-level TableConstraint entries.
        // perf: move the table out before normalization to avoid cloning the full table twice.
        let table_to_normalize = std::mem::replace(
            tbl,
            TableDef {
                name: table.to_string().into(),
                description: None,
                columns: Vec::new(),
                constraints: Vec::new(),
            },
        );
        let normalized = table_to_normalize.normalize().map_err(|e| {
            PlannerError::TableValidation(format!(
                "Failed to normalize table '{}' after adding column '{}': {}",
                table, column.name, e
            ))
        })?;
        *tbl = normalized;
        Ok(())
    }
}

pub(super) fn delete_column(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    let before = tbl.columns.len();
    tbl.columns.retain(|c| c.name != column);
    if tbl.columns.len() == before {
        Err(PlannerError::ColumnNotFound(
            table.to_string(),
            column.to_string(),
        ))
    } else {
        drop_column_from_constraints(&mut tbl.constraints, column);
        Ok(())
    }
}

pub(super) fn rename_column(
    schema: &mut [TableDef],
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), PlannerError> {
    let tbl = find_table_mut(schema, table)?;
    let col = tbl
        .columns
        .iter_mut()
        .find(|c| c.name == from)
        .ok_or_else(|| PlannerError::ColumnNotFound(table.to_string(), from.to_string()))?;
    col.name = to.into();
    rename_column_in_constraints(&mut tbl.constraints, from, to);
    Ok(())
}

pub(super) fn modify_column_type(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    new_type: &ColumnType,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.r#type = new_type.clone();
    Ok(())
}

/// Rewrite the stored `value` of every integer-enum variant whose current
/// value appears as a key in `mapping`. The column type and variant names
/// are left untouched; only the numeric values shift. No-op when the
/// column is not an integer enum (defensive — the diff layer should never
/// emit `RemapEnumValues` for non-integer-enum columns, but apply must not
/// panic in that case).
pub(super) fn remap_enum_values(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    mapping: &BTreeMap<i64, i64>,
) -> Result<(), PlannerError> {
    let col = find_column_mut(schema, table, column)?;
    if let ColumnType::Complex(ComplexColumnType::Enum {
        values: EnumValues::Integer(items),
        ..
    }) = &mut col.r#type
    {
        for item in items.iter_mut() {
            if let Some(&new_val) = mapping.get(&item.value) {
                item.value = new_val;
            }
        }
    }
    Ok(())
}

pub(super) fn modify_column_nullable(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    nullable: bool,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.nullable = nullable;
    Ok(())
}

pub(super) fn modify_column_default(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    new_default: Option<&str>,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.default = new_default.map(Into::into);
    Ok(())
}

pub(super) fn modify_column_comment(
    schema: &mut [TableDef],
    table: &str,
    column: &str,
    new_comment: Option<&String>,
) -> Result<(), PlannerError> {
    find_column_mut(schema, table, column)?.comment = new_comment.cloned();
    Ok(())
}

fn find_table_mut<'a>(
    schema: &'a mut [TableDef],
    table: &str,
) -> Result<&'a mut TableDef, PlannerError> {
    schema
        .iter_mut()
        .find(|t| t.name == table)
        .ok_or_else(|| PlannerError::TableNotFound(table.to_string()))
}

fn find_column_mut<'a>(
    schema: &'a mut [TableDef],
    table: &str,
    column: &str,
) -> Result<&'a mut ColumnDef, PlannerError> {
    find_table_mut(schema, table)?
        .columns
        .iter_mut()
        .find(|c| c.name == column)
        .ok_or_else(|| PlannerError::ColumnNotFound(table.to_string(), column.to_string()))
}

fn rename_column_in_constraints(constraints: &mut [TableConstraint], from: &str, to: &str) {
    for constraint in constraints {
        match constraint {
            TableConstraint::PrimaryKey { columns, .. }
            | TableConstraint::Unique { columns, .. }
            | TableConstraint::Index { columns, .. } => rename_column_refs(columns, from, to),
            TableConstraint::ForeignKey {
                columns,
                ref_columns,
                ..
            } => {
                rename_column_refs(columns, from, to);
                rename_column_refs(ref_columns, from, to);
            }
            TableConstraint::Check { .. } => {}
            _ => {
                unreachable!("TableConstraint is #[non_exhaustive]; all variants are matched above")
            }
        }
    }
}

fn rename_column_refs(columns: &mut [ColumnName], from: &str, to: &str) {
    for c in columns {
        if c == from {
            *c = to.into();
        }
    }
}

fn drop_column_from_constraints(constraints: &mut Vec<TableConstraint>, column: &str) {
    constraints.retain_mut(|c| match c {
        TableConstraint::PrimaryKey { columns, .. }
        | TableConstraint::Unique { columns, .. }
        | TableConstraint::Index { columns, .. } => {
            columns.retain(|c| c != column);
            !columns.is_empty()
        }
        TableConstraint::ForeignKey { columns, .. } => {
            columns.retain(|c| c != column);
            !columns.is_empty()
        }
        // `TableConstraint::Check` plus any future `#[non_exhaustive]`
        // variant: retain by default (no column reference to scrub).
        _ => true,
    });
}

#[cfg(test)]
pub(super) fn rename_column_in_constraints_for_test(
    constraints: &mut [TableConstraint],
    from: &str,
    to: &str,
) {
    rename_column_in_constraints(constraints, from, to);
}

#[cfg(test)]
mod tests {
    //! Coverage-closure tests targeting branches inside the private
    //! `column_ops::*` helpers that are exercised through `apply_action`
    //! but not always landed on by the existing higher-level tests.

    use super::*;
    use rstest::rstest;
    use vespertide_core::{
        ColumnDef, ColumnType, EnumValues, NumValue, SimpleColumnType, TableDef,
    };

    fn simple_col(name: &str, ty: SimpleColumnType) -> ColumnDef {
        ColumnDef::new(name, ColumnType::Simple(ty), true)
    }

    fn table_def(name: &str, columns: Vec<ColumnDef>) -> TableDef {
        TableDef {
            name: name.into(),
            description: None,
            columns,
            constraints: Vec::new(),
        }
    }

    // ── delete_column ──────────────────────────────────────────────────

    /// Coverage: lines 53–58 — `if tbl.columns.len() == before` Err arm
    /// returns `ColumnNotFound` when the named column isn't present.
    #[test]
    fn delete_column_missing_column_returns_column_not_found() {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("id", SimpleColumnType::Integer)],
        )];
        let err = delete_column(&mut schema, "users", "missing").unwrap_err();
        assert!(matches!(
            err,
            PlannerError::ColumnNotFound(ref t, ref c) if t == "users" && c == "missing"
        ));
    }

    /// Coverage: lines 59–61 — successful `delete_column` retains other
    /// columns and runs `drop_column_from_constraints`.
    #[test]
    fn delete_column_success_drops_only_named_column() {
        let mut schema = vec![table_def(
            "users",
            vec![
                simple_col("id", SimpleColumnType::Integer),
                simple_col("email", SimpleColumnType::Text),
            ],
        )];
        delete_column(&mut schema, "users", "email").unwrap();
        let cols: Vec<_> = schema[0]
            .columns
            .iter()
            .map(|c| c.name.to_string())
            .collect();
        assert_eq!(cols, vec!["id".to_string()]);
    }

    /// Coverage: `find_table_mut` Err propagation through `delete_column`.
    #[test]
    fn delete_column_unknown_table_returns_table_not_found() {
        let mut schema: Vec<TableDef> = vec![];
        let err = delete_column(&mut schema, "missing_table", "id").unwrap_err();
        assert!(matches!(
            err,
            PlannerError::TableNotFound(ref t) if t == "missing_table"
        ));
    }

    // ── rename_column ─────────────────────────────────────────────────

    /// Coverage: `rename_column` success path, including
    /// `rename_column_in_constraints` invocation.
    #[test]
    fn rename_column_success_renames_in_columns_and_constraints() {
        let mut schema = vec![TableDef {
            name: "users".into(),
            description: None,
            columns: vec![simple_col("email", SimpleColumnType::Text)],
            constraints: vec![TableConstraint::Index {
                name: Some("ix_users_email".into()),
                columns: vec!["email".into()],
            }],
        }];
        rename_column(&mut schema, "users", "email", "email_address").unwrap();
        assert_eq!(schema[0].columns[0].name.as_str(), "email_address");
        let TableConstraint::Index { columns, .. } = &schema[0].constraints[0] else {
            panic!("expected Index constraint");
        };
        assert_eq!(columns[0].as_str(), "email_address");
    }

    /// Coverage: `rename_column` ColumnNotFound branch.
    #[test]
    fn rename_column_missing_returns_column_not_found() {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("id", SimpleColumnType::Integer)],
        )];
        let err = rename_column(&mut schema, "users", "missing", "renamed").unwrap_err();
        assert!(matches!(err, PlannerError::ColumnNotFound(_, _)));
    }

    // ── modify_column_type ────────────────────────────────────────────

    #[test]
    fn modify_column_type_success_rewrites_column_type() {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("id", SimpleColumnType::Integer)],
        )];
        modify_column_type(
            &mut schema,
            "users",
            "id",
            &ColumnType::Simple(SimpleColumnType::BigInt),
        )
        .unwrap();
        assert_eq!(
            schema[0].columns[0].r#type,
            ColumnType::Simple(SimpleColumnType::BigInt)
        );
    }

    #[test]
    fn modify_column_type_missing_column_errors() {
        let mut schema = vec![table_def("users", vec![])];
        let err = modify_column_type(
            &mut schema,
            "users",
            "missing",
            &ColumnType::Simple(SimpleColumnType::Text),
        )
        .unwrap_err();
        assert!(matches!(err, PlannerError::ColumnNotFound(_, _)));
    }

    // ── modify_column_nullable ────────────────────────────────────────

    /// Coverage: line 124 — `find_column_mut(...)?.nullable = nullable;`
    #[rstest]
    #[case(true)]
    #[case(false)]
    fn modify_column_nullable_sets_flag(#[case] target: bool) {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("email", SimpleColumnType::Text)],
        )];
        // initial nullable from `simple_col` is true
        modify_column_nullable(&mut schema, "users", "email", target).unwrap();
        assert_eq!(schema[0].columns[0].nullable, target);
    }

    #[test]
    fn modify_column_nullable_missing_column_errors() {
        let mut schema = vec![table_def("users", vec![])];
        let err = modify_column_nullable(&mut schema, "users", "email", false).unwrap_err();
        assert!(matches!(err, PlannerError::ColumnNotFound(_, _)));
    }

    // ── modify_column_default / modify_column_comment ─────────────────

    #[test]
    fn modify_column_default_sets_and_clears() {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("status", SimpleColumnType::Text)],
        )];
        modify_column_default(&mut schema, "users", "status", Some("'active'")).unwrap();
        assert!(schema[0].columns[0].default.is_some());
        modify_column_default(&mut schema, "users", "status", None).unwrap();
        assert!(schema[0].columns[0].default.is_none());
    }

    #[test]
    fn modify_column_default_missing_column_errors() {
        let mut schema = vec![table_def("users", vec![])];
        let err = modify_column_default(&mut schema, "users", "status", Some("'x'")).unwrap_err();
        assert!(matches!(err, PlannerError::ColumnNotFound(_, _)));
    }

    #[test]
    fn modify_column_comment_sets_and_clears() {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("status", SimpleColumnType::Text)],
        )];
        let some = "User status".to_string();
        modify_column_comment(&mut schema, "users", "status", Some(&some)).unwrap();
        assert_eq!(schema[0].columns[0].comment.as_deref(), Some("User status"));
        modify_column_comment(&mut schema, "users", "status", None).unwrap();
        assert!(schema[0].columns[0].comment.is_none());
    }

    #[test]
    fn modify_column_comment_missing_column_errors() {
        let mut schema = vec![table_def("users", vec![])];
        let err = modify_column_comment(&mut schema, "users", "status", None).unwrap_err();
        assert!(matches!(err, PlannerError::ColumnNotFound(_, _)));
    }

    // ── remap_enum_values ─────────────────────────────────────────────

    /// `remap_enum_values` on a non-integer-enum column is a documented
    /// no-op (defensive: diff layer never emits this action for non-
    /// integer-enum columns).
    #[test]
    fn remap_enum_values_noop_on_non_integer_enum_column() {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("status", SimpleColumnType::Text)],
        )];
        let mut map = std::collections::BTreeMap::new();
        map.insert(0_i64, 100_i64);
        remap_enum_values(&mut schema, "users", "status", &map).unwrap();
        // Type unchanged.
        assert_eq!(
            schema[0].columns[0].r#type,
            ColumnType::Simple(SimpleColumnType::Text)
        );
    }

    /// `remap_enum_values` rewrites the stored numeric value of every
    /// matching variant.
    #[test]
    fn remap_enum_values_rewrites_integer_enum_values() {
        let mut col = ColumnDef::new(
            "priority",
            ColumnType::Complex(vespertide_core::ComplexColumnType::Enum {
                name: "priority_level".into(),
                values: EnumValues::Integer(vec![
                    NumValue {
                        name: "low".into(),
                        value: 0,
                    },
                    NumValue {
                        name: "high".into(),
                        value: 100,
                    },
                ]),
            }),
            false,
        );
        col.nullable = false;
        let mut schema = vec![table_def("orders", vec![col])];
        let mut map = std::collections::BTreeMap::new();
        map.insert(100_i64, 200_i64);
        remap_enum_values(&mut schema, "orders", "priority", &map).unwrap();
        let ColumnType::Complex(vespertide_core::ComplexColumnType::Enum {
            values: EnumValues::Integer(items),
            ..
        }) = &schema[0].columns[0].r#type
        else {
            panic!("expected integer enum");
        };
        assert_eq!(items[0].value, 0);
        assert_eq!(items[1].value, 200);
    }

    // ── add_column normalization edge ─────────────────────────────────

    /// `add_column` rejects duplicate name with `ColumnExists`.
    #[test]
    fn add_column_duplicate_returns_column_exists() {
        let mut schema = vec![table_def(
            "users",
            vec![simple_col("id", SimpleColumnType::Integer)],
        )];
        let err = add_column(
            &mut schema,
            "users",
            &simple_col("id", SimpleColumnType::Integer),
        )
        .unwrap_err();
        assert!(matches!(err, PlannerError::ColumnExists(_, _)));
    }
}
