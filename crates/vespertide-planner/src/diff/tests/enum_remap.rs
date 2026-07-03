//! F7-(b) integer enum value remap detection tests.
//!
//! `diff_integer_enum_remappings` lives in `crates/vespertide-planner/src/diff/columns.rs`
//! and runs as part of `diff_columns`. These tests drive it end-to-end via
//! the public `diff_schemas` entry so we exercise the same normalisation +
//! action-ordering path that production callers go through.

use crate::diff_schemas;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, MigrationAction, NumValue,
    TableConstraint, TableDef,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pk_int_col(name: &str) -> ColumnDef {
    let mut c = ColumnDef::new(
        name,
        ColumnType::Simple(vespertide_core::SimpleColumnType::Integer),
        false,
    );
    c.primary_key = Some(vespertide_core::schema::primary_key::PrimaryKeySyntax::Bool(true));
    c
}

fn enum_col(name: &str, ty_name: &str, items: Vec<(&str, i64)>) -> ColumnDef {
    let mut c = ColumnDef::new(
        name,
        ColumnType::Complex(ComplexColumnType::Enum {
            name: ty_name.to_string(),
            values: EnumValues::Integer(
                items
                    .into_iter()
                    .map(|(n, v)| NumValue {
                        name: n.to_string(),
                        value: v,
                    })
                    .collect(),
            ),
        }),
        false,
    );
    c.default = Some("0".into());
    c
}

fn table(name: &str, payload: ColumnDef) -> TableDef {
    TableDef {
        name: name.into(),
        description: None,
        columns: vec![pk_int_col("id"), payload],
        constraints: vec![TableConstraint::PrimaryKey {
            auto_increment: false,
            columns: vec!["id".into()],
            strategy: vespertide_core::PrimaryKeyAdditionStrategy::default(),
        }],
    }
}

fn diff_pair(from: TableDef, to: TableDef) -> Vec<MigrationAction> {
    diff_schemas(&[from], &[to]).unwrap().actions
}

fn remap_actions(actions: &[MigrationAction]) -> Vec<(String, Vec<(i64, i64)>)> {
    actions
        .iter()
        .filter_map(|a| match a {
            MigrationAction::RemapEnumValues {
                column, mapping, ..
            } => Some((
                column.to_string(),
                // Convert BTreeMap back to a Vec<(i64,i64)> so the existing
                // tests can keep asserting on a sequence (the underlying
                // type changed in 0.2.x for typed-uniqueness; the test
                // shape is preserved).
                mapping.iter().map(|(k, v)| (*k, *v)).collect(),
            )),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Detection: shifted value pairs
// ---------------------------------------------------------------------------

#[test]
fn detects_single_shifted_value() {
    let from = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5), ("high", 10)],
        ),
    );
    let to = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 100), ("high", 10)],
        ),
    );
    let actions = diff_pair(from, to);
    let remaps = remap_actions(&actions);
    assert_eq!(remaps.len(), 1);
    assert_eq!(remaps[0].0, "priority");
    assert_eq!(remaps[0].1, vec![(5, 100)]);
}

#[test]
fn detects_multiple_shifted_values_sorted_by_old() {
    let from = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5), ("high", 10)],
        ),
    );
    let to = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 100), ("high", 200)],
        ),
    );
    let actions = diff_pair(from, to);
    let remaps = remap_actions(&actions);
    assert_eq!(remaps.len(), 1);
    assert_eq!(
        remaps[0].1,
        vec![(5, 100), (10, 200)],
        "mapping must be sorted by old value"
    );
}

#[test]
fn value_swap_is_detected() {
    // medium and high exchange numeric values. Both pairs land in mapping;
    // the SQL generator's atomic CASE WHEN handles the swap correctly.
    let from = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5), ("high", 10)],
        ),
    );
    let to = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 10), ("high", 5)],
        ),
    );
    let actions = diff_pair(from, to);
    let remaps = remap_actions(&actions);
    assert_eq!(remaps.len(), 1);
    assert_eq!(remaps[0].1, vec![(5, 10), (10, 5)]);
}

// ---------------------------------------------------------------------------
// No-op: unchanged / additive / removal / non-integer
// ---------------------------------------------------------------------------

#[test]
fn unchanged_values_emit_no_remap() {
    let from = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5)],
        ),
    );
    let to = from.clone();
    let actions = diff_pair(from, to);
    assert!(remap_actions(&actions).is_empty());
}

#[test]
fn variant_added_is_not_a_remap() {
    let from = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5)],
        ),
    );
    let to = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5), ("high", 10)],
        ),
    );
    let actions = diff_pair(from, to);
    assert!(
        remap_actions(&actions).is_empty(),
        "new variant is additive — no remap"
    );
}

#[test]
fn variant_removed_is_not_a_remap() {
    let from = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5), ("high", 10)],
        ),
    );
    let to = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5)],
        ),
    );
    let actions = diff_pair(from, to);
    assert!(
        remap_actions(&actions).is_empty(),
        "removal is handled by other paths"
    );
}

#[test]
fn variant_renamed_with_same_value_is_not_a_remap() {
    let from = table(
        "tickets",
        enum_col("priority", "ticket_priority", vec![("low", 0), ("med", 5)]),
    );
    let to = table(
        "tickets",
        enum_col(
            "priority",
            "ticket_priority",
            vec![("low", 0), ("medium", 5)],
        ),
    );
    let actions = diff_pair(from, to);
    // No shared name with shifted value — `med` removed, `medium` added.
    assert!(remap_actions(&actions).is_empty());
}

#[test]
fn string_enum_is_ignored_by_this_pass() {
    use vespertide_core::EnumValues;
    let from_payload = {
        let mut c = ColumnDef::new(
            "status",
            ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec!["a".into(), "b".into()]),
            }),
            false,
        );
        c.default = Some("'a'".into());
        c
    };
    let to_payload = {
        let mut c = ColumnDef::new(
            "status",
            ColumnType::Complex(ComplexColumnType::Enum {
                name: "status".into(),
                values: EnumValues::String(vec!["a".into(), "c".into()]),
            }),
            false,
        );
        c.default = Some("'a'".into());
        c
    };
    let from = table("orders", from_payload);
    let to = table("orders", to_payload);
    let actions = diff_pair(from, to);
    assert!(
        remap_actions(&actions).is_empty(),
        "string enums are handled by ModifyColumnType + fill_with"
    );
}
