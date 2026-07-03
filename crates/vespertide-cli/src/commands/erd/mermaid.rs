use std::fmt::Write as _;

use vespertide_core::{ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, TableDef};

use super::{
    Cardinality, ForeignKeyRelation, collect_foreign_key_relations, is_foreign_key_column,
    is_primary_key_column, sanitize_identifier,
};

pub fn render_mermaid(tables: &[TableDef]) -> String {
    let mut output = String::from("erDiagram\n");

    for table in tables {
        writeln!(output, "  {} {{", sanitize_identifier(&table.name))
            .expect("write Mermaid table header");

        for column in &table.columns {
            let primary_key = if is_primary_key_column(table, &column.name) {
                " PK"
            } else {
                ""
            };
            let foreign_key = if is_foreign_key_column(table, &column.name) {
                " FK"
            } else {
                ""
            };

            writeln!(
                output,
                "    {} {}{}{}",
                column_type_to_mermaid(&column.r#type),
                sanitize_identifier(&column.name),
                primary_key,
                foreign_key
            )
            .expect("write Mermaid column");
        }

        writeln!(output, "  }}").expect("write Mermaid table footer");
    }

    for relation in collect_foreign_key_relations(tables) {
        let (left_table, connector, right_table) = mermaid_relationship(&relation);
        writeln!(
            output,
            "  {} {} {} : \"{}\"",
            sanitize_identifier(left_table),
            connector,
            sanitize_identifier(right_table),
            escape_mermaid_label(&relation.child_columns.join(", "))
        )
        .expect("write Mermaid relationship");
    }

    output
}

fn mermaid_relationship(relation: &ForeignKeyRelation) -> (&str, &'static str, &str) {
    match relation.cardinality {
        Cardinality::OneToOne => (&relation.parent_table, "||--||", &relation.child_table),
        Cardinality::OneToMany => (&relation.parent_table, "||--o{", &relation.child_table),
        Cardinality::ZeroOrOneToMany => (&relation.parent_table, "|o--o{", &relation.child_table),
        Cardinality::ManyToMany => (&relation.child_table, "}o--||", &relation.parent_table),
    }
}

fn column_type_to_mermaid(column_type: &ColumnType) -> &'static str {
    match column_type {
        ColumnType::Simple(simple) => simple_column_type_to_mermaid(*simple),
        ColumnType::Complex(complex) => complex_column_type_to_mermaid(complex),
    }
}

fn simple_column_type_to_mermaid(column_type: SimpleColumnType) -> &'static str {
    match column_type {
        SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt => "int",
        SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "float",
        SimpleColumnType::Boolean => "boolean",
        SimpleColumnType::Date => "date",
        SimpleColumnType::Time => "time",
        SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => "datetime",
        SimpleColumnType::Bytea => "binary",
        SimpleColumnType::Uuid => "uuid",
        SimpleColumnType::Json => "json",
        _ => "string",
    }
}

fn complex_column_type_to_mermaid(column_type: &ComplexColumnType) -> &'static str {
    match column_type {
        ComplexColumnType::Numeric { .. } => "decimal",
        ComplexColumnType::Enum { values, .. } => match values {
            EnumValues::String(_) => "string",
            EnumValues::Integer(_) => "int",
        },
        _ => "string",
    }
}

fn escape_mermaid_label(label: &str) -> String {
    label.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    //! Coverage for `column_type_to_mermaid` dispatch arms (lines 42-69).
    //! High-level mermaid snapshots only exercise integer / text — the
    //! float / boolean / date / time / timestamp / bytea / uuid / json /
    //! numeric / enum-string / enum-integer / unknown-simple / unknown-complex
    //! arms need explicit fixtures.
    use super::*;
    use vespertide_core::{ComplexColumnType, EnumValues, NumValue, SimpleColumnType};

    #[test]
    fn simple_column_type_to_mermaid_covers_every_arm() {
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::SmallInt),
            "int"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Integer),
            "int"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::BigInt),
            "int"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Real),
            "float"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::DoublePrecision),
            "float"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Boolean),
            "boolean"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Date),
            "date"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Time),
            "time"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Timestamp),
            "datetime"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Timestamptz),
            "datetime"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Bytea),
            "binary"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Uuid),
            "uuid"
        );
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Json),
            "json"
        );
        // Wildcard arm — any non-listed simple type falls back to "string".
        assert_eq!(
            simple_column_type_to_mermaid(SimpleColumnType::Text),
            "string"
        );
    }

    #[test]
    fn complex_column_type_to_mermaid_covers_every_arm() {
        let numeric = ComplexColumnType::Numeric {
            precision: 10,
            scale: 2,
        };
        assert_eq!(complex_column_type_to_mermaid(&numeric), "decimal");
        let string_enum = ComplexColumnType::Enum {
            name: "status".into(),
            values: EnumValues::String(vec!["a".into(), "b".into()]),
        };
        assert_eq!(complex_column_type_to_mermaid(&string_enum), "string");
        let int_enum = ComplexColumnType::Enum {
            name: "prio".into(),
            values: EnumValues::Integer(vec![NumValue {
                name: "low".into(),
                value: 0,
            }]),
        };
        assert_eq!(complex_column_type_to_mermaid(&int_enum), "int");
        // Wildcard — `varchar` / `char` / `custom` fall back to "string".
        let varchar = ComplexColumnType::Varchar { length: 10 };
        assert_eq!(complex_column_type_to_mermaid(&varchar), "string");
    }

    #[test]
    fn column_type_to_mermaid_dispatches_simple_and_complex() {
        // Simple arm (line 42)
        assert_eq!(
            column_type_to_mermaid(&ColumnType::Simple(SimpleColumnType::Integer)),
            "int"
        );
        // Complex arm (line 43)
        assert_eq!(
            column_type_to_mermaid(&ColumnType::Complex(ComplexColumnType::Numeric {
                precision: 5,
                scale: 2
            })),
            "decimal"
        );
    }

    #[test]
    fn escape_mermaid_label_escapes_backslash_and_quote() {
        assert_eq!(escape_mermaid_label("a\\b\"c"), "a\\\\b\\\"c");
    }
}
