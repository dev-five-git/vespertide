use std::collections::{HashMap, HashSet};

use vespertide_core::TableDef;
use vespertide_core::schema::column::{ColumnType, ComplexColumnType, EnumValues};
use vespertide_naming::{
    IdentifierStart, build_enum_type_name, sanitize_identifier, to_pascal_case,
    to_screaming_snake_case,
};

/// Bare `PascalCase` identifiers that the schema declares with more than one
/// value set.
///
/// Every other backend writes one file per table, so a repeated enum is
/// naturally scoped; Prisma emits a single file, where reusing an identifier
/// would silently give both tables the first table's values. Names are compared
/// *after* the case conversion, since distinct names can collapse onto the same
/// identifier (`doc_status` and `docStatus` are both `DocStatus`).
pub(super) fn ambiguous_enum_identifiers(schema: &[TableDef]) -> HashSet<String> {
    let mut declared: HashMap<String, &EnumValues> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for table in schema {
        for (name, values) in collect_table_enums(table) {
            let identifier = to_pascal_case(name);
            if let Some(first) = declared.get(&identifier) {
                if *first != values {
                    ambiguous.insert(identifier);
                }
            } else {
                declared.insert(identifier, values);
            }
        }
    }
    ambiguous
}

/// Prisma identifier for a table's enum.
///
/// Ambiguous identifiers fall back to the database type name (`{table}_{enum}`,
/// the same one the SQL layer creates), so the Prisma enum maps 1:1 onto the
/// type the column actually has.
pub(super) fn enum_identifier(table: &str, name: &str, ambiguous: &HashSet<String>) -> String {
    let bare = to_pascal_case(name);
    let chosen = if ambiguous.contains(&bare) {
        to_pascal_case(&build_enum_type_name(table, name))
    } else {
        bare
    };
    sanitize_identifier(&chosen, IdentifierStart::Letter)
}

/// Enum columns of a table, first declaration wins per name.
pub(super) fn collect_table_enums(table: &TableDef) -> Vec<(&str, &EnumValues)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for col in &table.columns {
        if let ColumnType::Complex(ComplexColumnType::Enum { name, values }) = &col.r#type
            && seen.insert(name.as_str())
        {
            result.push((name.as_str(), values));
        }
    }
    result
}

/// Prisma identifier for one enum variant.
///
/// Prisma's parser rejects a leading `_`, so a value that starts with a digit is
/// escaped with a letter rather than an underscore. The original value survives
/// in the `@map` the caller emits.
pub(super) fn enum_variant(value: &str) -> String {
    sanitize_identifier(&to_screaming_snake_case(value), IdentifierStart::Letter)
}

/// Render one enum block under an already-resolved identifier, i.e. the output
/// of [`enum_identifier`] rather than the raw enum name.
pub(super) fn render_enum(identifier: &str, values: &EnumValues) -> String {
    let mut lines = Vec::new();
    lines.push(format!("enum {identifier} {{"));
    match values {
        EnumValues::String(vals) => {
            for val in vals {
                let variant = enum_variant(val);
                if variant == *val {
                    lines.push(format!("  {variant}"));
                } else {
                    lines.push(format!("  {variant} @map(\"{val}\")"));
                }
            }
        }
        EnumValues::Integer(vals) => {
            // Prisma doesn't support integer enums natively; emit as string variants with comment
            for val in vals {
                let variant = enum_variant(&val.name);
                let value = val.value;
                lines.push(format!("  {variant} // = {value}"));
            }
        }
    }
    lines.push("}".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use vespertide_core::schema::column::NumValue;

    use super::*;

    #[rstest]
    #[case::already_screaming(
        vec!["DRAFT".into(), "PUBLISHED".into()],
        "enum DocStatus {\n  DRAFT\n  PUBLISHED\n}"
    )]
    #[case::normalized(
        vec!["draft".into(), "in progress".into()],
        "enum DocStatus {\n  DRAFT @map(\"draft\")\n  IN_PROGRESS @map(\"in progress\")\n}"
    )]
    // `_1CRITICAL` would be rejected by Prisma's parser, so the escape is a
    // letter; `@map` still carries the value the database stores.
    #[case::leading_digit(
        vec!["1critical".into()],
        "enum DocStatus {\n  X1CRITICAL @map(\"1critical\")\n}"
    )]
    fn string_variants_carry_map_only_when_normalization_changes_them(
        #[case] values: Vec<String>,
        #[case] expected: &str,
    ) {
        assert_eq!(
            render_enum("DocStatus", &EnumValues::String(values)),
            expected
        );
    }

    #[test]
    fn integer_variants_keep_the_declared_value_in_a_comment() {
        let values = EnumValues::Integer(vec![
            NumValue {
                name: "low".into(),
                value: 100,
            },
            NumValue {
                name: "high".into(),
                value: 200,
            },
        ]);
        assert_eq!(
            render_enum("Priority", &values),
            "enum Priority {\n  LOW // = 100\n  HIGH // = 200\n}"
        );
    }
}
