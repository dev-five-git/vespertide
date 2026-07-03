use insta::{assert_snapshot, with_settings};
use rstest::rstest;
use vespertide_core::TableDef;

use crate::orm::{Orm, render_entity, render_entity_with_schema};

pub(crate) mod fixtures;

/// Dispatch the per-ORM **multi-table** entry point so the cross-ORM
/// `orm_cases!(multi ...)` arm renders a `Vec<TableDef>` schema for all four
/// ORMs through a single call. JPA's `render_entities` returns `Vec<String>`
/// (one entry per entity); we join with `"\n"` to match the
/// `String`-returning shape of the other three.
fn render_schema(orm: Orm, schema: &[TableDef]) -> Result<String, String> {
    match orm {
        Orm::SeaOrm => crate::seaorm::export(schema),
        Orm::SqlAlchemy => crate::sqlalchemy::export(schema),
        Orm::SqlModel => crate::sqlmodel::render_entities(schema),
        Orm::Jpa => crate::jpa::render_entities(schema).map(|entities| entities.join("\n")),
    }
}

macro_rules! orm_cases {
    // Single-table variant — fixture returns one `TableDef`; renders via
    // `render_entity(orm, &table)`.
    ($test_name:ident, $scenario:literal, $fixture:path) => {
        #[rstest]
        #[case::seaorm(Orm::SeaOrm)]
        #[case::sqlalchemy(Orm::SqlAlchemy)]
        #[case::sqlmodel(Orm::SqlModel)]
        #[case::jpa(Orm::Jpa)]
        fn $test_name(#[case] orm: Orm) {
            let table = $fixture();
            let rendered = render_entity(orm, &table).unwrap();
            with_settings!({ snapshot_suffix => format!("{}_{:?}", $scenario, orm) }, {
                assert_snapshot!(rendered);
            });
        }
    };
    // Multi-table variant — fixture returns `Vec<TableDef>`; renders via the
    // per-ORM multi-table entry point dispatched by `render_schema`.
    (multi $test_name:ident, $scenario:literal, $fixture:path) => {
        #[rstest]
        #[case::seaorm(Orm::SeaOrm)]
        #[case::sqlalchemy(Orm::SqlAlchemy)]
        #[case::sqlmodel(Orm::SqlModel)]
        #[case::jpa(Orm::Jpa)]
        fn $test_name(#[case] orm: Orm) {
            let schema: Vec<TableDef> = $fixture();
            let rendered = render_schema(orm, &schema).unwrap();
            with_settings!({ snapshot_suffix => format!("{}_{:?}", $scenario, orm) }, {
                assert_snapshot!(rendered);
            });
        }
    };
}

orm_cases!(
    basic_table_with_description_snapshot,
    "basic_table_with_description",
    fixtures::basic_table_with_description
);
orm_cases!(
    basic_single_pk_snapshot,
    "basic_single_pk",
    fixtures::basic_single_pk
);
orm_cases!(
    composite_pk_snapshot,
    "composite_pk",
    fixtures::composite_pk
);
orm_cases!(
    table_with_fk_snapshot,
    "table_with_fk",
    fixtures::table_with_fk
);
orm_cases!(
    table_with_composite_fk_snapshot,
    "table_with_composite_fk",
    fixtures::table_with_composite_fk
);
orm_cases!(inline_pk_snapshot, "inline_pk", fixtures::inline_pk);
orm_cases!(
    pk_and_fk_together_snapshot,
    "pk_and_fk_together",
    fixtures::pk_and_fk_together
);
orm_cases!(
    table_with_enum_snapshot,
    "table_with_enum",
    fixtures::table_with_enum
);
orm_cases!(
    table_with_integer_enum_snapshot,
    "table_with_integer_enum",
    fixtures::table_with_integer_enum
);
orm_cases!(
    nullable_enum_snapshot,
    "nullable_enum",
    fixtures::nullable_enum
);
orm_cases!(
    enum_multiple_columns_snapshot,
    "enum_multiple_columns",
    fixtures::enum_multiple_columns
);
orm_cases!(enum_shared_snapshot, "enum_shared", fixtures::enum_shared);
orm_cases!(
    enum_special_values_snapshot,
    "enum_special_values",
    fixtures::enum_special_values
);
orm_cases!(
    enum_with_default_snapshot,
    "enum_with_default",
    fixtures::enum_with_default
);
orm_cases!(
    unique_and_indexed_snapshot,
    "unique_and_indexed",
    fixtures::unique_and_indexed
);
orm_cases!(
    table_with_indexes_snapshot,
    "table_with_indexes",
    fixtures::table_with_indexes
);
orm_cases!(
    table_level_pk_snapshot,
    "table_level_pk",
    fixtures::table_level_pk
);
orm_cases!(
    all_simple_types_snapshot,
    "all_simple_types",
    fixtures::all_simple_types
);
orm_cases!(
    complex_types_snapshot,
    "complex_types",
    fixtures::complex_types
);
orm_cases!(
    jsonb_custom_type_snapshot,
    "jsonb_custom_type",
    fixtures::jsonb_custom_type
);
orm_cases!(defaults_snapshot, "defaults", fixtures::defaults);
orm_cases!(
    server_defaults_snapshot,
    "server_defaults",
    fixtures::server_defaults
);
orm_cases!(
    server_default_and_true_boolean_snapshot,
    "server_default_and_true_boolean",
    fixtures::server_default_and_true_boolean
);
orm_cases!(
    nullable_columns_snapshot,
    "nullable_columns",
    fixtures::nullable_columns
);
orm_cases!(
    composite_constraints_snapshot,
    "composite_constraints",
    fixtures::composite_constraints
);
orm_cases!(
    composite_unique_snapshot,
    "composite_unique",
    fixtures::composite_unique
);
orm_cases!(
    composite_index_snapshot,
    "composite_index",
    fixtures::composite_index
);
orm_cases!(
    unnamed_index_and_unique_snapshot,
    "unnamed_index_and_unique",
    fixtures::unnamed_index_and_unique
);
orm_cases!(
    unnamed_composite_index_snapshot,
    "unnamed_composite_index",
    fixtures::unnamed_composite_index
);
orm_cases!(
    unnamed_composite_unique_snapshot,
    "unnamed_composite_unique",
    fixtures::unnamed_composite_unique
);
orm_cases!(
    no_description_snapshot,
    "no_description",
    fixtures::no_description
);
orm_cases!(
    string_default_snapshot,
    "string_default",
    fixtures::string_default
);
orm_cases!(
    false_boolean_default_snapshot,
    "false_boolean_default",
    fixtures::false_boolean_default
);
orm_cases!(
    unknown_function_default_snapshot,
    "unknown_function_default",
    fixtures::unknown_function_default
);
orm_cases!(
    unknown_constant_default_snapshot,
    "unknown_constant_default",
    fixtures::unknown_constant_default
);
orm_cases!(
    fk_with_comment_and_auto_increment_snapshot,
    "fk_with_comment_and_auto_increment",
    fixtures::fk_with_comment_and_auto_increment
);
orm_cases!(
    json_default_snapshot,
    "json_default",
    fixtures::json_default
);
orm_cases!(
    self_referencing_fk_snapshot,
    "self_referencing_fk",
    fixtures::self_referencing_fk
);
orm_cases!(
    reserved_word_identifiers_snapshot,
    "reserved_word_identifiers",
    fixtures::reserved_word_identifiers
);
orm_cases!(
    composite_primary_key_snapshot,
    "composite_primary_key",
    fixtures::composite_primary_key
);
orm_cases!(
    composite_unique_constraint_snapshot,
    "composite_unique_constraint",
    fixtures::composite_unique_constraint
);
orm_cases!(
    integer_enum_all_variant_types_snapshot,
    "integer_enum_all_variant_types",
    fixtures::integer_enum_all_variant_types
);
orm_cases!(
    numeric_default_value_snapshot,
    "numeric_default_value",
    fixtures::numeric_default_value
);
orm_cases!(
    integer_enum_with_default_snapshot,
    "integer_enum_with_default",
    fixtures::integer_enum_with_default
);
// Cross-ORM coverage closure for the variant-name branch of
// `seaorm/types.rs` `format_default_value` (the `else` arm inside
// `EnumValues::Integer(int_values) =>`, lines 47-60). The existing
// `integer_enum_with_default` fixture covers the numeric-literal `if` arm via
// `default("1")`; this fixture exercises the lookup arm via
// `default("Completed")` → resolves to value `100`.
orm_cases!(
    integer_enum_with_variant_default_snapshot,
    "integer_enum_with_variant_default",
    fixtures::integer_enum_with_variant_default
);
// Cross-ORM coverage closure for the **sequential** branch of every per-ORM
// multi-table entry point:
// * `sqlalchemy/render.rs` lines 21-29 (`pub fn export`)
// * `sqlmodel/render.rs` lines 62-65, 84-92, 94-100
//   (`pub fn render_entities` + `render_entities_sequential`)
// * `seaorm::export` + `jpa::render_entities` sequential arms
// The existing `tests/parallel_consolidated.rs` integration test exercises
// the parallel branch with a 100-table schema; this scenario completes the
// matrix by exercising the < 50-table sequential branch through the shared
// `orm_cases!` macro (multi-table variant).
orm_cases!(
    multi small_multi_schema_sequential_snapshot,
    "small_multi_schema_sequential",
    fixtures::small_multi_schema
);

/// Dispatch the per-ORM `to_pascal_case` helper from a single entry point so
/// the cross-ORM consolidation test can exercise all four implementations
/// without leaking the helper as a generally-public crate API.
fn to_pascal_case_for(orm: Orm, s: &str) -> String {
    match orm {
        Orm::SeaOrm => crate::seaorm::to_pascal_case_for_tests(s),
        Orm::SqlAlchemy => crate::sqlalchemy::to_pascal_case_for_tests(s),
        Orm::SqlModel => crate::sqlmodel::to_pascal_case_for_tests(s),
        Orm::Jpa => crate::jpa::to_pascal_case_for_tests(s),
    }
}

/// Cross-ORM `to_pascal_case` consolidation. Inputs in this matrix are
/// restricted to ASCII with `_` as the only separator — the subset where all
/// four ORM implementations agree.
///
/// Divergences intentionally NOT covered here:
/// * `-` as separator: `SeaORM` treats it as a separator, the other three
///   ORMs leave it intact (their splits operate on `_` only).
/// * Non-ASCII characters: `SeaORM` uses `to_ascii_uppercase`, the others
///   use `to_uppercase` (Unicode-aware).
/// These divergences are exercised in the per-ORM `tests.rs` files where
/// applicable.
#[rstest]
#[case::seaorm(Orm::SeaOrm)]
#[case::sqlalchemy(Orm::SqlAlchemy)]
#[case::sqlmodel(Orm::SqlModel)]
#[case::jpa(Orm::Jpa)]
fn to_pascal_case_shared_semantics(
    #[values(
        ("", ""),
        ("a", "A"),
        ("abc", "Abc"),
        ("simple", "Simple"),
        ("users", "Users"),
        ("hello_world", "HelloWorld"),
        ("user_id", "UserId"),
        ("a_b_c", "ABC"),
        ("a__b", "AB"),
        ("order_item", "OrderItem"),
        ("user_profile_image", "UserProfileImage")
    )]
    case: (&str, &str),
    #[case] orm: Orm,
) {
    let (input, expected) = case;
    assert_eq!(to_pascal_case_for(orm, input), expected);
}

#[rstest]
#[case::seaorm(Orm::SeaOrm)]
#[case::sqlalchemy(Orm::SqlAlchemy)]
#[case::sqlmodel(Orm::SqlModel)]
#[case::jpa(Orm::Jpa)]
fn render_entity_with_schema_snapshots(
    #[values(
        "many_to_many_article",
        "many_to_many_user",
        "many_to_many_missing_target",
        "many_to_many_multiple_junctions",
        "composite_fk_parent",
        "not_junction_single_pk",
        "not_junction_fk_not_in_pk_other",
        "not_junction_fk_not_in_pk_another",
        "multiple_fk_same_table",
        "username_fk",
        "multiple_reverse_relations",
        "dual_reverse_relations",
        "triple_reverse_relations",
        "multiple_has_one_relations"
    )]
    scenario: &str,
    #[case] orm: Orm,
) {
    let (table, schema) = fixtures::schema_scenario(scenario);
    let rendered = render_entity_with_schema(orm, &table, &schema).unwrap();
    with_settings!({ snapshot_suffix => format!("{}_{:?}", scenario, orm) }, {
        assert_snapshot!(rendered);
    });
}
