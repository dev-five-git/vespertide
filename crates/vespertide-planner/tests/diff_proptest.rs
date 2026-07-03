use proptest::prelude::*;
use vespertide_core::{ColumnDef, ColumnType, SimpleColumnType, TableConstraint, TableDef};
use vespertide_planner::{apply_action, diff_schemas};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// For any acyclic schema pair (from, to), applying diff(from, to) must
    /// make the replayed schema equivalent to to, modulo normalization.
    #[test]
    fn diff_apply_produces_target_schema((from, to) in arb_schema_pair_acyclic()) {
        let Ok(plan) = diff_schemas(&from, &to) else {
            return Ok(());
        };

        let mut replay = from.clone();
        for action in &plan.actions {
            apply_action(&mut replay, action).expect("apply must succeed on valid plan");
        }

        let re_diff = diff_schemas(&replay, &to).expect("re-diff must succeed");
        prop_assert!(
            re_diff.actions.is_empty(),
            "expected no-op re-diff, got {} actions:\n{:?}",
            re_diff.actions.len(),
            re_diff.actions
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// diff(s, s) must be empty for any acyclic schema s.
    #[test]
    fn diff_self_is_empty(s in arb_schema_acyclic()) {
        if let Ok(plan) = diff_schemas(&s, &s) {
            prop_assert!(plan.actions.is_empty(), "diff(s,s) produced {:?}", plan.actions);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Applying generated actions must preserve per-table column-name uniqueness.
    #[test]
    fn apply_preserves_column_uniqueness((from, to) in arb_schema_pair_acyclic()) {
        if let Ok(plan) = diff_schemas(&from, &to) {
            let mut replay = from.clone();
            for action in &plan.actions {
                let _ = apply_action(&mut replay, action);
                for table in &replay {
                    prop_assert!(table.validate_unique_column_names().is_ok());
                }
            }
        }
    }
}

/// Generates a vector of `TableDef`s with unique names and an acyclic FK graph.
fn arb_schema_acyclic() -> impl Strategy<Value = Vec<TableDef>> {
    prop::collection::vec(arb_table_def(), 1..6)
        .prop_filter("unique table names", |tables| {
            let mut seen = std::collections::BTreeSet::new();
            tables.iter().all(|table| seen.insert(table.name.clone()))
        })
        .prop_map(|mut tables| {
            for table in &mut tables {
                table
                    .constraints
                    .retain(|constraint| !matches!(constraint, TableConstraint::ForeignKey { .. }));
                for column in &mut table.columns {
                    column.foreign_key = None;
                }
            }
            tables
        })
}

/// Pair of acyclic schemas where `to` differs from `from` by a small mutation.
fn arb_schema_pair_acyclic() -> impl Strategy<Value = (Vec<TableDef>, Vec<TableDef>)> {
    arb_schema_acyclic().prop_flat_map(|from| {
        let target_seed = from.clone();
        (Just(from), mutate_schema(&target_seed))
    })
}

/// Applies a small mutation to produce a related schema.
fn mutate_schema(from: &[TableDef]) -> impl Strategy<Value = Vec<TableDef>> + use<> {
    prop_oneof![
        Just(from.to_owned()),
        (0usize..from.len().max(1)).prop_map({
            let from = from.to_owned();
            move |index| {
                let mut to = from.clone();
                if index < to.len() {
                    to.remove(index);
                }
                to
            }
        }),
        (arb_safe_ident(), 0usize..from.len().max(1)).prop_map({
            let from = from.to_owned();
            move |(new_name, index)| {
                let mut to = from.clone();
                if let Some(table) = to.get_mut(index)
                    && !table
                        .columns
                        .iter()
                        .any(|column| column.name.as_str() == new_name)
                {
                    table.columns.push(ColumnDef::new(
                        new_name,
                        ColumnType::Simple(SimpleColumnType::Text),
                        true,
                    ));
                }
                to
            }
        }),
    ]
}

// TODO: deduplicate with vespertide-core::arbitrary once available.
fn arb_table_def() -> impl Strategy<Value = TableDef> {
    (arb_safe_ident(), arb_unique_columns()).prop_map(|(name, columns)| TableDef {
        name: name.into(),
        description: None,
        columns,
        constraints: Vec::new(),
    })
}

// TODO: deduplicate with vespertide-core::arbitrary once available.
fn arb_unique_columns() -> impl Strategy<Value = Vec<ColumnDef>> {
    prop::collection::vec(arb_column_def(), 1..6).prop_filter("unique column names", |columns| {
        let mut seen = std::collections::BTreeSet::new();
        columns
            .iter()
            .all(|column| seen.insert(column.name.clone()))
    })
}

// TODO: deduplicate with vespertide-core::arbitrary once available.
fn arb_column_def() -> impl Strategy<Value = ColumnDef> {
    (arb_safe_ident(), arb_simple_column_type(), any::<bool>())
        .prop_map(|(name, column_type, nullable)| ColumnDef::new(name, column_type, nullable))
}

// TODO: deduplicate with vespertide-core::arbitrary once available.
fn arb_simple_column_type() -> impl Strategy<Value = ColumnType> {
    prop_oneof![
        Just(SimpleColumnType::SmallInt),
        Just(SimpleColumnType::Integer),
        Just(SimpleColumnType::BigInt),
        Just(SimpleColumnType::Real),
        Just(SimpleColumnType::DoublePrecision),
        Just(SimpleColumnType::Text),
        Just(SimpleColumnType::Boolean),
        Just(SimpleColumnType::Date),
        Just(SimpleColumnType::Time),
        Just(SimpleColumnType::Timestamp),
        Just(SimpleColumnType::Timestamptz),
        Just(SimpleColumnType::Interval),
        Just(SimpleColumnType::Bytea),
        Just(SimpleColumnType::Uuid),
        Just(SimpleColumnType::Json),
        Just(SimpleColumnType::Inet),
        Just(SimpleColumnType::Cidr),
        Just(SimpleColumnType::Macaddr),
        Just(SimpleColumnType::Xml),
    ]
    .prop_map(ColumnType::Simple)
}

// TODO: deduplicate with vespertide-core::arbitrary once available.
fn arb_safe_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,10}"
}
