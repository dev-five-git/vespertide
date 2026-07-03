//! Naming helpers for relation enums, fields, and FK attribute rendering.

use std::collections::HashSet;

use super::super::imports::{sanitize_field_name, to_pascal_case};

/// Generate a relation enum name from foreign key column names.
/// For "`creator_user_id`", generates "`CreatorUser`".
/// For composite FKs like [`org_id`, `user_id`], generates `OrgUser`.
pub(in crate::seaorm) fn generate_relation_enum_name<T: AsRef<str>>(columns: &[T]) -> String {
    // Take the first column and remove common FK suffixes like "_id"
    let first_col = columns[0].as_ref();
    let without_id = if let Some(stripped) = first_col.strip_suffix("_id") {
        stripped
    } else {
        first_col
    };

    to_pascal_case(without_id)
}

pub(in crate::seaorm) fn unique_relation_enum_name(
    preferred: String,
    source_table: &str,
    base_relation_enum: &str,
    used_relation_enums: &HashSet<String>,
) -> String {
    if !used_relation_enums.contains(&preferred) {
        return preferred;
    }

    let source_prefixed = format!("{}{}", to_pascal_case(source_table), base_relation_enum);
    if !used_relation_enums.contains(&source_prefixed) {
        return source_prefixed;
    }

    let mut index = 2;
    loop {
        let candidate = format!(
            "{}{}{}",
            to_pascal_case(source_table),
            base_relation_enum,
            index
        );
        if !used_relation_enums.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

/// Infer a field name from a single FK column.
/// For "`creator_user_id`" with to="id", tries "`creator_user`" first.
/// If the FK column still follows common suffix naming like `_id`/`_idx`,
/// remove those as fallbacks for intuitive relation names.
/// If that ends with the table name, use the full column name (without the to suffix).
/// Otherwise, fall back to the table name.
///
/// Examples:
/// - FK column: "`creator_user_id`", table: "user", to: "id" -> "`creator_user`"
/// - FK column: "`creator_user_idx`", table: "user", to: "idx" -> "`creator_user`"
/// - FK column: "`user_id`", table: "user", to: "id" -> "user" (falls back to table name)
/// - FK column: "`order_id`", table: "order", to: "`order_number`" -> "order"
/// - FK column: "`order_idx`", table: "order", to: "`order_number`" -> "order"
/// - FK column: "`org_id`", table: "user", to: "id" -> "org"
pub(in crate::seaorm) fn infer_field_name_from_fk_column(
    fk_column: &str,
    table_name: &str,
    to: &str,
) -> String {
    let table_lower = table_name.to_lowercase();
    let to_lower = to.to_lowercase();

    // Remove the "to" suffix from FK column (e.g., "user_id" for to="id", "user_idx" for to="idx").
    // If FK column still uses common suffixes like "*_id"/"*_idx", strip them as fallbacks.
    let to_suffix = format!("_{to}");
    let without_suffix = fk_column
        .strip_suffix(&to_suffix)
        .or_else(|| fk_column.strip_suffix("_id"))
        .or_else(|| fk_column.strip_suffix("_idx"))
        .unwrap_or(fk_column);

    let sanitized = sanitize_field_name(without_suffix);
    let sanitized_lower = sanitized.to_lowercase();

    // If the FK column exactly matches the referenced column name, treat it as a natural-key
    // relation and expose the target entity name instead of the raw column name.
    // Also handle compact forms like `username` for `user.name`.
    if sanitized_lower == to_lower || sanitized_lower == format!("{table_lower}{to_lower}") {
        return sanitize_field_name(table_name);
    }

    // If the sanitized name is exactly the table name (e.g., "user_id" -> "user" for table "user"),
    // we need to fall back to the table name for proper disambiguation
    if sanitized_lower == table_lower {
        sanitize_field_name(table_name)
    }
    // If the sanitized name ends with (but is not equal to) the table name, use it as-is
    // This handles cases like "creator_user" for table "user"
    else if sanitized_lower.ends_with(&table_lower) {
        sanitized
    } else {
        // Otherwise, use the inferred name from the column
        sanitized
    }
}

pub(in crate::seaorm) fn pluralize(name: &str) -> String {
    if name.ends_with('s') || name.ends_with("es") {
        name.to_string()
    } else if name.ends_with('y')
        && !name.ends_with("ay")
        && !name.ends_with("ey")
        && !name.ends_with("oy")
        && !name.ends_with("uy")
    {
        format!("{}ies", name.strip_suffix('y').unwrap_or(name))
    } else {
        format!("{name}s")
    }
}

pub(super) fn fk_attr_value<T: AsRef<str>>(cols: &[T]) -> String {
    if cols.len() == 1 {
        cols[0].as_ref().to_string()
    } else {
        let joined = cols
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ");
        format!("({joined})")
    }
}
