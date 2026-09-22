use std::collections::HashSet;

use vespertide_core::schema::column::EnumValues;

use crate::enum_scan::variant_names;
use crate::utils::common::{claim_binding, string_literal};
use crate::utils::python::enum_member_name;

/// Members carry only their value. Django derives the human label from the
/// member name (`PENDING` -> "Pending"); passing the raw database value as an
/// explicit label would pin a worse one ("pending").
pub(super) fn render_enum(lines: &mut Vec<String>, class_name: &str, values: &EnumValues) {
    let members = member_names(values);
    match values {
        EnumValues::String(vals) => {
            lines.push(format!("class {class_name}(models.TextChoices):"));
            for (val, member) in vals.iter().zip(members) {
                lines.push(format!("    {member} = {}", string_literal(val)));
            }
        }
        EnumValues::Integer(vals) => {
            lines.push(format!("class {class_name}(models.IntegerChoices):"));
            for (val, member) in vals.iter().zip(members) {
                lines.push(format!("    {member} = {}", val.value));
            }
        }
    }
}

/// One member name per variant, in order. Distinct values can fold onto one
/// name (`in progress`, `in-progress`), and Python's `Enum` refuses to define
/// a member twice — at import, which takes the whole module down.
fn member_names(values: &EnumValues) -> Vec<String> {
    let mut taken = HashSet::new();
    variant_names(values)
        .into_iter()
        .map(|variant| claim_binding(enum_member_name(variant), &mut taken))
        .collect()
}

#[cfg(test)]
mod tests {
    use vespertide_core::schema::column::EnumValues;

    use super::member_names;

    #[test]
    fn values_that_fold_onto_one_member_name_are_numbered() {
        let values = EnumValues::String(vec![
            "in progress".into(),
            "in-progress".into(),
            "done".into(),
        ]);
        assert_eq!(
            member_names(&values),
            ["IN_PROGRESS", "IN_PROGRESS2", "DONE"]
        );
    }
}
