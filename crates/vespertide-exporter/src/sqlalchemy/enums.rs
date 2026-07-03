use super::render::{to_pascal_case, to_screaming_snake_case};
use vespertide_core::schema::column::EnumValues;

pub(super) fn render_enum(lines: &mut Vec<String>, name: &str, values: &EnumValues) {
    let class_name = to_pascal_case(name);

    match values {
        EnumValues::String(vals) => {
            lines.push(format!("class {class_name}(str, enum.Enum):"));
            for val in vals {
                let variant_name = to_screaming_snake_case(val);
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
