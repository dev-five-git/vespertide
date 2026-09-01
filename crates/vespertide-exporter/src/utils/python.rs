use vespertide_core::TableDef;
use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
use vespertide_core::schema::constraint::TableConstraint;

use vespertide_naming::{IdentifierStart, sanitize_identifier, to_screaming_snake_case};

use crate::python_naming::to_pascal_case;

/// Emit a Python `enum` class definition shared verbatim by the SQLAlchemy and
/// SQLModel backends: `class {Pascal}(str, enum.Enum)` for string enums (members
/// via `to_screaming_snake_case`) and `class {Pascal}(enum.IntEnum)` for integer
/// enums. Both Python ORMs produce byte-identical enum classes.
pub(crate) fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    let class_name = to_pascal_case(name);

    match values {
        EnumValues::String(vals) => {
            lines.push(format!("class {class_name}(str, enum.Enum):"));
            for val in vals {
                // Python accepts a leading `_` in a member name, so the
                // digit escape is `_` rather than the letter Prisma needs.
                let variant_name =
                    sanitize_identifier(&to_screaming_snake_case(val), IdentifierStart::Underscore);
                lines.push(format!("    {variant_name} = \"{val}\""));
            }
        }
        EnumValues::Integer(vals) => {
            lines.push(format!("class {class_name}(enum.IntEnum):"));
            for val in vals {
                lines.push(format!("    {} = {}", val.name, val.value));
            }
        }
    }
}

/// Map a `ColumnType` to its Python type annotation string, shared verbatim by
/// the SQLAlchemy and SQLModel backends (both produce identical
/// `int`/`float`/`str`/`datetime`/`Decimal`/`Optional[...]`/enum-PascalCase
/// annotations). A single home means a future `SimpleColumnType` /
/// `ComplexColumnType` variant is mapped in exactly one place.
pub(crate) fn column_type_to_python(col_type: &ColumnType, nullable: bool) -> String {
    let base = match col_type {
        ColumnType::Simple(ty) => match ty {
            SimpleColumnType::SmallInt | SimpleColumnType::Integer | SimpleColumnType::BigInt => {
                "int"
            }
            SimpleColumnType::Real | SimpleColumnType::DoublePrecision => "float",
            SimpleColumnType::Text
            | SimpleColumnType::Interval
            | SimpleColumnType::Inet
            | SimpleColumnType::Cidr
            | SimpleColumnType::Macaddr
            | SimpleColumnType::Xml => "str",
            SimpleColumnType::Boolean => "bool",
            SimpleColumnType::Date => "date",
            SimpleColumnType::Time => "time",
            SimpleColumnType::Timestamp | SimpleColumnType::Timestamptz => "datetime",
            SimpleColumnType::Bytea => "bytes",
            SimpleColumnType::Uuid => "UUID",
            SimpleColumnType::Json => "dict",
            _ => unreachable!(
                "SimpleColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
        ColumnType::Complex(ty) => match ty {
            ComplexColumnType::Numeric { .. } => "Decimal",
            ComplexColumnType::Varchar { .. }
            | ComplexColumnType::Char { .. }
            | ComplexColumnType::Custom { .. } => "str",
            ComplexColumnType::Enum { name, .. } => {
                return if nullable {
                    format!("Optional[{}]", to_pascal_case(name))
                } else {
                    to_pascal_case(name)
                };
            }
            _ => unreachable!(
                "ComplexColumnType is #[non_exhaustive]; all variants are matched above"
            ),
        },
    };

    if nullable {
        format!("Optional[{base}]")
    } else {
        base.to_string()
    }
}

pub(crate) struct CompositeFk<'a> {
    pub local_cols: Vec<&'a str>,
    pub ref_table: &'a str,
    pub ref_cols: Vec<&'a str>,
}

pub(crate) fn collect_composite_fks(table: &TableDef) -> Vec<CompositeFk<'_>> {
    table
        .constraints
        .iter()
        .filter_map(|constraint| match constraint {
            TableConstraint::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                ..
            } if columns.len() > 1 && columns.len() == ref_columns.len() => Some(CompositeFk {
                local_cols: columns.iter().map(AsRef::as_ref).collect(),
                ref_table: ref_table.as_str(),
                ref_cols: ref_columns.iter().map(AsRef::as_ref).collect(),
            }),
            _ => None,
        })
        .collect()
}
