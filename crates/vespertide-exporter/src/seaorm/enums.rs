use vespertide_config::SeaOrmConfig;
use vespertide_core::{EnumValues, NumValue};

use super::imports::{sanitize_type_name, to_pascal_case};

pub(super) fn render_enum(
    lines: &mut Vec<String>,
    table_name: &str,
    name: &str,
    values: &EnumValues,
    config: &SeaOrmConfig,
) {
    let enum_name = to_pascal_case(name);
    // Construct the full enum name with table prefix for database
    let db_enum_name = format!("{table_name}_{name}");

    // Build derive line with optional extra derives
    let mut derives = vec![
        "Debug",
        "Clone",
        "PartialEq",
        "Eq",
        "EnumIter",
        "DeriveActiveEnum",
        "Serialize",
        "Deserialize",
    ];
    let extra_derives: Vec<&str> = config
        .extra_enum_derives()
        .iter()
        .map(std::string::String::as_str)
        .collect();
    derives.extend(extra_derives);

    lines.push(format!("#[derive({})]", derives.join(", ")));
    lines.push(format!(
        "#[serde(rename_all = \"{}\")]",
        enum_serde_rename_all(values, config)
    ));

    match values {
        EnumValues::Integer(_) => {
            // Integer enum: #[sea_orm(rs_type = "i32", db_type = "Integer")]
            lines.push("#[sea_orm(rs_type = \"i32\", db_type = \"Integer\")]".into());
        }
        EnumValues::String(_) => {
            // String enum: #[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "...")]
            lines.push(format!("#[sea_orm(rs_type = \"String\", db_type = \"Enum\", enum_name = \"{db_enum_name}\")]"));
        }
    }

    lines.push(format!("pub enum {enum_name} {{"));

    match values {
        EnumValues::String(string_values) => {
            let use_screaming_snake_variants = uses_screaming_snake_variants(string_values);
            for s in string_values {
                let variant_name = enum_string_variant_name(s, use_screaming_snake_variants);
                lines.push(format!("    #[sea_orm(string_value = \"{s}\")]"));
                lines.push(format!("    {variant_name},"));
            }
        }
        EnumValues::Integer(int_values) => {
            for NumValue {
                name: var_name,
                value: num,
            } in int_values
            {
                let variant_name = enum_variant_name(var_name);
                lines.push(format!("    {variant_name} = {num},"));
            }
        }
    }
    lines.push("}".into());
    lines.push(String::new());
}

/// Convert a string to a valid Rust enum variant name (`PascalCase`).
/// Handles edge cases like numeric prefixes, special characters, and reserved words.
pub(super) fn enum_variant_name(s: &str) -> String {
    let pascal = to_pascal_case(s);

    finalize_enum_variant_name(pascal)
}

pub(super) fn enum_string_variant_name(s: &str, use_screaming_snake_variants: bool) -> String {
    let pascal = if use_screaming_snake_variants {
        screaming_snake_to_pascal_case(s)
    } else {
        to_pascal_case(s)
    };

    finalize_enum_variant_name(pascal)
}

pub(super) fn finalize_enum_variant_name(pascal: String) -> String {
    // Handle empty string
    if pascal.is_empty() {
        return "Value".to_string();
    }

    // Handle numeric prefix: prefix with underscore or 'N'
    let pascal = if pascal.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("N{pascal}")
    } else {
        pascal
    };

    // `to_pascal_case` splits on `_` and `-` only, so anything else a value may
    // carry — a space, a non-ASCII letter — is still sitting in the name.
    sanitize_type_name(&pascal)
}

pub(super) fn enum_serde_rename_all(values: &EnumValues, config: &SeaOrmConfig) -> &'static str {
    match values {
        EnumValues::String(string_values) if uses_screaming_snake_variants(string_values) => {
            "SCREAMING_SNAKE_CASE"
        }
        _ => config.enum_naming_case().serde_rename_all(),
    }
}

pub(super) fn uses_screaming_snake_variants(values: &[String]) -> bool {
    !values.is_empty() && values.iter().all(|value| is_screaming_snake_value(value))
}

pub(super) fn is_screaming_snake_value(value: &str) -> bool {
    let mut has_ascii_upper = false;

    for ch in value.chars() {
        if ch.is_ascii_lowercase() {
            return false;
        }
        if ch.is_ascii_uppercase() {
            has_ascii_upper = true;
            continue;
        }
        if ch.is_ascii_digit() || ch == '_' {
            continue;
        }
        return false;
    }

    has_ascii_upper
}

pub(super) fn screaming_snake_to_pascal_case(value: &str) -> String {
    let pascal: String = value
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            let first = chars
                .next()
                .expect("empty segments are filtered before PascalCase conversion");
            let mut out = String::new();
            out.push(first.to_ascii_uppercase());
            out.extend(chars.map(|ch| ch.to_ascii_lowercase()));
            out
        })
        .collect();

    if pascal.is_empty() {
        "Value".to_string()
    } else {
        pascal
    }
}
