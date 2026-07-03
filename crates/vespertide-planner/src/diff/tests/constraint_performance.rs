use super::*;

#[test]
#[expect(
    clippy::print_stderr,
    reason = "performance regression test emits elapsed time only when test output is requested"
)]
fn diff_constraint_replacement_is_linear_per_table() {
    let columns = (0..100)
        .map(|i| {
            col(
                &format!("col_{i}"),
                ColumnType::Simple(SimpleColumnType::Integer),
            )
        })
        .collect::<Vec<_>>();

    let from_constraints = (0..100)
        .map(|i| TableConstraint::Index {
            name: Some(format!("ix_source_{i}")),
            columns: vec![format!("col_{i}").into()],
        })
        .collect::<Vec<_>>();
    let to_constraints = (0..100)
        .map(|i| TableConstraint::Index {
            name: Some(format!("ix_target_{i}")),
            columns: vec![format!("col_{i}").into()],
        })
        .collect::<Vec<_>>();

    let from = vec![table("sample", columns.clone(), from_constraints)];
    let to = vec![table("sample", columns, to_constraints)];

    let start = std::time::Instant::now();
    let plan = diff_schemas(&from, &to).unwrap();
    let elapsed = start.elapsed();
    eprintln!("constraint replacement diff elapsed: {elapsed:?}");

    assert!(elapsed < std::time::Duration::from_secs(1));
    assert_eq!(100, plan.actions.len());
    assert!(plan.actions.iter().all(|action| matches!(
        action,
        MigrationAction::ReplaceConstraint { .. }
            | MigrationAction::RemoveConstraint { .. }
            | MigrationAction::AddConstraint { .. }
    )));
}
