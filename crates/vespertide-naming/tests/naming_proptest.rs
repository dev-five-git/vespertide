use proptest::prelude::*;
use vespertide_naming::{build_foreign_key_name, build_index_name, build_unique_constraint_name};

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    }
}

// Safe identifier strategy.
fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,20}"
}

fn arb_table_and_columns() -> impl Strategy<Value = (String, Vec<String>)> {
    (arb_ident(), prop::collection::vec(arb_ident(), 1..6)).prop_filter(
        "unique columns",
        |(_, cols)| {
            let mut seen = std::collections::BTreeSet::new();
            cols.iter().all(|c| seen.insert(c.clone()))
        },
    )
}

proptest! {
    #![proptest_config(config())]

    /// Reordering the input column list must NOT change the generated index name.
    #[test]
    fn build_index_name_is_order_independent((table, mut cols) in arb_table_and_columns()) {
        let name1 = build_index_name(&table, &cols, None);
        cols.reverse();
        let name2 = build_index_name(&table, &cols, None);
        prop_assert_eq!(name1, name2);
    }

    /// Same for unique constraint name.
    #[test]
    fn build_unique_constraint_name_is_order_independent(
        (table, mut cols) in arb_table_and_columns(),
    ) {
        let name1 = build_unique_constraint_name(&table, &cols, None);
        cols.reverse();
        let name2 = build_unique_constraint_name(&table, &cols, None);
        prop_assert_eq!(name1, name2);
    }

    /// Same for foreign key name.
    #[test]
    fn build_foreign_key_name_is_order_independent(
        (table, mut cols) in arb_table_and_columns(),
    ) {
        let name1 = build_foreign_key_name(&table, &cols, None);
        cols.reverse();
        let name2 = build_foreign_key_name(&table, &cols, None);
        prop_assert_eq!(name1, name2);
    }

    /// Generated names must follow the documented prefix convention.
    #[test]
    fn name_format_invariants((table, cols) in arb_table_and_columns()) {
        let ix = build_index_name(&table, &cols, None);
        let uq = build_unique_constraint_name(&table, &cols, None);
        let fk = build_foreign_key_name(&table, &cols, None);
        prop_assert!(ix.starts_with("ix_"), "index name should start with ix_: {ix}");
        prop_assert!(uq.starts_with("uq_"), "unique name should start with uq_: {uq}");
        prop_assert!(fk.starts_with("fk_"), "fk name should start with fk_: {fk}");
        prop_assert!(ix.contains(&table), "table name should appear: {ix}");
        prop_assert!(uq.contains(&table), "table name should appear: {uq}");
        prop_assert!(fk.contains(&table), "table name should appear: {fk}");
    }

    /// Permutation invariance: ANY permutation of columns produces the same name.
    #[test]
    fn name_invariant_under_permutation(
        (table, cols) in arb_table_and_columns(),
        seed: u64,
    ) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut shuffled = cols.clone();
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let h = hasher.finish();
        shuffled.sort_by_key(|c| {
            let mut hh = DefaultHasher::new();
            (c, h).hash(&mut hh);
            hh.finish()
        });
        let name1 = build_index_name(&table, &cols, None);
        let name2 = build_index_name(&table, &shuffled, None);
        prop_assert_eq!(name1, name2);
    }
}
