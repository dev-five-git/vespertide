use super::*;
use vespertide_core::schema::primary_key::PrimaryKeySyntax;
use vespertide_core::{
    ColumnDef, ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, StringOrBool,
    TableConstraint, TableDef,
};

mod helpers;
mod misc;
mod module_path;
mod reverse_relation;

#[test]
fn render_entity_omits_eq_for_float_models() {
    let table = TableDef {
        name: "measurements".into(),
        description: None,
        columns: vec![
            ColumnDef::new("id", ColumnType::Simple(SimpleColumnType::Integer), false)
                .primary_key(PrimaryKeySyntax::Bool(true)),
            ColumnDef::new(
                "score",
                ColumnType::Simple(SimpleColumnType::DoublePrecision),
                false,
            ),
        ],
        constraints: vec![],
    };

    let rendered = render_entity(&table);
    assert!(rendered.contains("#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]"));
    assert!(!rendered.contains("#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]"));
}
