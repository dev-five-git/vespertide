use vespertide_core::schema::column::EnumValues;
use vespertide_naming::{IdentifierStart, sanitize_identifier};

use super::render::to_pascal_case;
use crate::utils::common::string_literal;

/// `type_name` and `const_names` are the names claimed for this enum in the
/// package's scope, one constant per value in declaration order.
pub(super) fn render_enum(
    lines: &mut Vec<String>,
    type_name: &str,
    const_names: &[String],
    values: &EnumValues,
) {
    let mut rendered = match values {
        EnumValues::String(_) => {
            vec![
                format!("type {type_name} string"),
                String::new(),
                "const (".into(),
            ]
        }
        EnumValues::Integer(_) => {
            vec![
                format!("type {type_name} int"),
                String::new(),
                "const (".into(),
            ]
        }
    };

    match values {
        EnumValues::String(vals) => {
            for (val, const_name) in vals.iter().zip(const_names) {
                rendered.push(format!(
                    "    {const_name} {type_name} = {}",
                    string_literal(val)
                ));
            }
        }
        EnumValues::Integer(vals) => {
            for (val, const_name) in vals.iter().zip(const_names) {
                rendered.push(format!("    {const_name} {type_name} = {}", val.value));
            }
        }
    }

    rendered.push(")".into());
    lines.extend(rendered);
}

/// The natural Go constant name for one enum member, before it is claimed in
/// the package's scope. The value is arbitrary text — `info-level` and
/// `1critical` are legal in the database — so it is escaped the same way
/// column names are. The `type_name` prefix already supplies a leading letter,
/// so only interior characters can need replacing.
pub(super) fn const_name(type_name: &str, value: &str) -> String {
    sanitize_identifier(
        &format!("{type_name}{}", to_pascal_case(value)),
        IdentifierStart::Letter,
    )
}
