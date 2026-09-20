use vespertide_core::schema::column::{
    ColumnType, ComplexColumnType, EnumValues, SimpleColumnType,
};
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
                let variant_name = enum_member_name(val);
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

/// Python's hard keywords (`keyword.kwlist`, 3.12). Soft keywords (`match`,
/// `case`, `type`, `_`) stay valid identifiers and need no escape.
const PYTHON_KEYWORDS: [&str; 35] = [
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

pub(crate) fn is_python_keyword(name: &str) -> bool {
    PYTHON_KEYWORDS.contains(&name)
}

/// PEP 8's escape for a name that is a Python keyword: a trailing `_`. The
/// callers already emit the database column name whenever the attribute
/// differs from it.
pub(crate) fn escape_python_keyword(mut name: String) -> String {
    if is_python_keyword(&name) {
        name.push('_');
    }
    name
}

/// Member name for a Python enum class: `SCREAMING_SNAKE_CASE` of the value.
/// Python accepts a leading `_` in a member name, so the digit escape is `_`
/// rather than the letter Prisma needs.
pub(crate) fn enum_member_name(value: &str) -> String {
    unmangled(sanitize_identifier(
        &to_screaming_snake_case(value),
        IdentifierStart::Underscore,
    ))
}

/// Inside a class body Python rewrites a name led by `__` into
/// `_Class__name`: an enum member spelled that way is no member, and a class
/// spelled that way cannot be named from another class. One `_` stays.
pub(crate) fn unmangled(name: String) -> String {
    let body = name.trim_start_matches('_');
    if name.len() - body.len() < 2 {
        return name;
    }
    format!("_{body}")
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::enum_member_name;

    #[rstest]
    #[case::plain("pending", "PENDING")]
    #[case::words("in progress", "IN_PROGRESS")]
    #[case::digit_led("1st", "_1ST")]
    #[case::one_leading_separator("-x", "_X")]
    #[case::leading_run_python_would_mangle("--x", "_X")]
    fn enum_values_become_member_names(#[case] value: &str, #[case] expected: &str) {
        assert_eq!(enum_member_name(value), expected);
    }
}
