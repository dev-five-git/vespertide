use vespertide_core::{MigrationPlan, TableDef};

use crate::error::PlannerError;
use crate::{diff_schemas, schema_from_plans};

/// Build the next migration plan given the current schema and existing plans.
/// The baseline schema is reconstructed from already-applied plans and then
/// diffed against the target `current` schema.
pub fn plan_next_migration(
    current: &[TableDef],
    applied_plans: &[MigrationPlan],
) -> Result<MigrationPlan, PlannerError> {
    let baseline = schema_from_plans(applied_plans)?;
    plan_next_migration_with_baseline(current, applied_plans, &baseline)
}

/// Build the next migration plan given the current schema, existing plans, and
/// a pre-computed baseline schema. This is more efficient when the baseline
/// schema is already available, avoiding redundant calls to `schema_from_plans`.
pub fn plan_next_migration_with_baseline(
    current: &[TableDef],
    applied_plans: &[MigrationPlan],
    baseline: &[TableDef],
) -> Result<MigrationPlan, PlannerError> {
    let mut plan = diff_schemas(baseline, current)?;

    let next_version = applied_plans
        .iter()
        .map(|p| p.version)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    plan.version = next_version;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use vespertide_core::{ColumnDef, ColumnType, MigrationAction, SimpleColumnType};

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            r#type: ty,
            nullable: true,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    fn table(
        name: &str,
        columns: Vec<ColumnDef>,
        constraints: Vec<vespertide_core::TableConstraint>,
    ) -> TableDef {
        TableDef {
            name: name.to_string(),
            description: None,
            columns,
            constraints,
        }
    }

    #[rstest]
    fn plan_next_migration_sets_next_version() {
        let applied = vec![MigrationPlan {
            comment: None,
            created_at: None,
            version: 1,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![col("id", ColumnType::Simple(SimpleColumnType::Integer))],
                constraints: vec![],
            }],
        }];

        let target_schema = vec![table(
            "users",
            vec![
                col("id", ColumnType::Simple(SimpleColumnType::Integer)),
                col("name", ColumnType::Simple(SimpleColumnType::Text)),
            ],
            vec![],
        )];

        let plan = plan_next_migration(&target_schema, &applied).unwrap();
        assert_eq!(plan.version, 2);
        assert!(plan.actions.iter().any(
            |a| matches!(a, MigrationAction::AddColumn { column, .. } if column.name == "name")
        ));
    }
}
