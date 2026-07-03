use vespertide_core::{ColumnType, ComplexColumnType, EnumValues, SimpleColumnType, StringOrBool};

/// Format default value for `SeaORM` attribute.
/// Returns the full attribute string like `default_value = "..."` or `default_value = 0`.
pub(super) fn format_default_value(value: &StringOrBool, column_type: &ColumnType) -> String {
    // Handle boolean values directly
    if let Some(b) = value.as_bool() {
        return format!("default_value = {b}");
    }

    // For string values, process as before
    let value_str = value.to_sql();
    let trimmed = value_str.trim();

    // Remove surrounding single quotes if present (SQL string literals)
    let cleaned = if let Some(inner) = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
    {
        inner
    } else {
        trimmed
    };

    // Escape double quotes for embedding in Rust attribute strings
    let escaped = cleaned.replace('"', "\\\"");

    // Format based on column type
    match column_type {
        // Numeric types: no quotes
        ColumnType::Simple(simple) if is_numeric_simple_type(*simple) => {
            format!("default_value = {cleaned}")
        }
        // Boolean type: no quotes
        // Numeric complex type: no quotes
        ColumnType::Simple(vespertide_core::SimpleColumnType::Boolean)
        | ColumnType::Complex(ComplexColumnType::Numeric { .. }) => {
            format!("default_value = {cleaned}")
        }
        // Enum type: use the actual database value (string or number), not Rust enum variant
        ColumnType::Complex(ComplexColumnType::Enum { values, .. }) => {
            match values {
                EnumValues::String(_) => {
                    // String enum: use the string value as-is with quotes
                    format!("default_value = \"{escaped}\"")
                }
                EnumValues::Integer(int_values) => {
                    // Integer enum: can be either a number or a variant name
                    // Try to parse as number first
                    if let Ok(num) = cleaned.parse::<i32>() {
                        // Already a number, use as-is
                        format!("default_value = {num}")
                    } else {
                        // It's a variant name, find the corresponding numeric value
                        let numeric_value = int_values
                            .iter()
                            .find(|v| v.name.eq_ignore_ascii_case(cleaned))
                            .map_or(0, |v| v.value); // Default to 0 if not found
                        format!("default_value = {numeric_value}")
                    }
                }
            }
        }
        // All other types: use quotes
        _ => {
            format!("default_value = \"{escaped}\"")
        }
    }
}

/// Check if the simple column type is numeric.
pub(super) fn is_numeric_simple_type(simple: vespertide_core::SimpleColumnType) -> bool {
    matches!(
        simple,
        SimpleColumnType::SmallInt
            | SimpleColumnType::Integer
            | SimpleColumnType::BigInt
            | SimpleColumnType::Real
            | SimpleColumnType::DoublePrecision
    )
}

pub(super) fn column_type_supports_eq(column_type: &ColumnType) -> bool {
    match column_type {
        ColumnType::Simple(SimpleColumnType::Real | SimpleColumnType::DoublePrecision) => false,
        ColumnType::Simple(_) | ColumnType::Complex(_) => true,
    }
}
