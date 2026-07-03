use std::collections::HashSet;

use vespertide_core::{ColumnDef, ColumnType, ComplexColumnType, EnumValues};

use crate::error::{InvalidEnumDefaultError, PlannerError};

/// Extract the unquoted value from a potentially quoted string.
/// Returns None if the value is a SQL expression (contains parentheses or is a keyword).
fn extract_enum_value(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Check for SQL expressions/keywords that shouldn't be validated
    if trimmed.contains('(')
        || trimmed.contains(')')
        || trimmed.eq_ignore_ascii_case("null")
        || trimmed.eq_ignore_ascii_case("current_timestamp")
        || trimmed.eq_ignore_ascii_case("now")
    {
        return None;
    }
    // Strip quotes if present
    if ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"')))
        && trimmed.len() >= 2
    {
        return trimmed
            .strip_prefix(['\'', '"'])
            .and_then(|s| s.strip_suffix(['\'', '"']));
    }
    // Unquoted value
    Some(trimmed)
}

/// Validate that an enum `default/fill_with` value is in the allowed enum values.
pub(super) fn validate_enum_value(
    value: &str,
    enum_name: &str,
    enum_values: &EnumValues,
    table_name: &str,
    column_name: &str,
    value_type: &str, // "default" or "fill_with"
) -> Result<(), PlannerError> {
    let Some(extracted) = extract_enum_value(value) else {
        return Ok(());
    };

    let is_valid = match enum_values {
        EnumValues::String(variants) => variants.iter().any(|v| v == extracted),
        EnumValues::Integer(variants) => extracted.parse::<i32>().map_or_else(
            |_| variants.iter().any(|v| v.name == extracted),
            |n| variants.iter().any(|v| v.value == i64::from(n)),
        ),
    };

    if !is_valid {
        let allowed = enum_values.variant_names().join(", ");
        return Err(Box::new(InvalidEnumDefaultError {
            enum_name: enum_name.to_string(),
            table_name: table_name.to_string(),
            column_name: column_name.to_string(),
            value_type: value_type.to_string(),
            value: extracted.to_string(),
            allowed,
        })
        .into());
    }

    Ok(())
}

pub(super) fn validate_column(column: &ColumnDef, table_name: &str) -> Result<(), PlannerError> {
    if let ColumnType::Complex(complex_type) = &column.r#type {
        match complex_type {
            ComplexColumnType::Numeric { precision, scale } if scale > precision => {
                return Err(PlannerError::TableValidation(format!(
                    "numeric column '{}.{}' scale ({scale}) must be <= precision ({precision})",
                    table_name, column.name
                )));
            }
            ComplexColumnType::Enum { name, values } => {
                match values {
                    EnumValues::String(variants) => {
                        let mut seen = HashSet::new();
                        for variant in variants {
                            if !seen.insert(variant.as_str()) {
                                return Err(PlannerError::DuplicateEnumVariantName(
                                    name.clone(),
                                    table_name.to_string(),
                                    column.name.to_string(),
                                    variant.clone(),
                                ));
                            }
                        }
                    }
                    EnumValues::Integer(variants) => {
                        let mut seen_names = HashSet::new();
                        for variant in variants {
                            if !seen_names.insert(variant.name.as_str()) {
                                return Err(PlannerError::DuplicateEnumVariantName(
                                    name.clone(),
                                    table_name.to_string(),
                                    column.name.to_string(),
                                    variant.name.clone(),
                                ));
                            }
                        }

                        for variant in variants {
                            if i32::try_from(variant.value).is_err() {
                                return Err(PlannerError::TableValidation(format!(
                                    "integer enum value {} is outside i32 range for enum '{}' in column '{}.{}'",
                                    variant.value, name, table_name, column.name
                                )));
                            }
                        }

                        let mut seen_values = HashSet::new();
                        for variant in variants {
                            if !seen_values.insert(variant.value) {
                                return Err(PlannerError::DuplicateEnumValue(
                                    name.clone(),
                                    table_name.to_string(),
                                    column.name.to_string(),
                                    variant.value,
                                ));
                            }
                        }
                    }
                }

                if let Some(default) = &column.default {
                    let default_str = default.to_sql();
                    validate_enum_value(
                        &default_str,
                        name,
                        values,
                        table_name,
                        &column.name,
                        "default",
                    )?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
