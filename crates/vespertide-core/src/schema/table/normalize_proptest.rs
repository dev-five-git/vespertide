#![cfg(feature = "arbitrary")]

use proptest::prelude::*;

use crate::arbitrary::arb_table_def;
use crate::schema::primary_key::PrimaryKeySyntax;
use crate::{ColumnDef, ColumnType, SimpleColumnType, StrOrBoolOrArray, TableDef};

fn normalize_again(table: &TableDef) -> TableDef {
    table
        .clone()
        .normalize()
        .expect("normalize must not fail on already-normalized output")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_shrink_iters: 5000,
        ..ProptestConfig::default()
    })]

    /// normalize() must be idempotent: applying it twice equals applying it once.
    #[test]
    fn normalize_is_idempotent(table in arb_table_def()) {
        if let Ok(once) = table.normalize() {
            let twice = normalize_again(&once);
            prop_assert_eq!(once, twice);
        }
        // If normalize fails (e.g. duplicate index name), the property is vacuously true.
    }
}

#[test]
fn normalize_is_idempotent_for_fixed_inline_constraints() {
    let table = TableDef {
        name: "account".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            ColumnDef::new("email", ColumnType::Simple(SimpleColumnType::Text), false)
                .unique(StrOrBoolOrArray::Bool(true))
                .index(StrOrBoolOrArray::Bool(true)),
        ],
        constraints: Vec::new(),
    };

    let once = table.normalize().expect("fixed table should normalize");
    let twice = normalize_again(&once);

    assert_eq!(once, twice);
}
