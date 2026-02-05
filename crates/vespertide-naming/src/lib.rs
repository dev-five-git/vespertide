//! Naming conventions and helpers for vespertide database schema management.
//!
//! This crate provides consistent naming functions for database objects like
//! indexes, constraints, and foreign keys. It has no dependencies and can be
//! used by any other vespertide crate.

// ============================================================================
// Relation Naming (for ORM exporters)
// ============================================================================

/// Extract semantic prefix from FK column for reverse relation naming.
///
/// Given an FK column name, the current (target) table name, and the referenced
/// column name (e.g., "id", "idx"), extracts the semantic role portion.
///
/// # Arguments
/// * `fk_column` - The FK column name (e.g., "user_id", "answered_by_user_id", "author_id")
/// * `current_table` - The table being referenced (e.g., "user")
/// * `ref_column` - The referenced column name (e.g., "id", "idx", "pk")
///
/// # Returns
/// The semantic prefix (empty string for default FK, or the role/prefix for others)
///
/// # Examples
/// ```
/// use vespertide_naming::extract_relation_prefix;
///
/// // Default FK: column matches table name + ref_column suffix
/// assert_eq!(extract_relation_prefix("user_id", "user", "id"), "");
/// assert_eq!(extract_relation_prefix("user_idx", "user", "idx"), "");
///
/// // Prefixed FK: has semantic prefix before table name
/// assert_eq!(extract_relation_prefix("answered_by_user_id", "user", "id"), "answered_by");
/// assert_eq!(extract_relation_prefix("target_user_id", "user", "id"), "target");
///
/// // Role FK: column doesn't end with table name
/// assert_eq!(extract_relation_prefix("author_id", "user", "id"), "author");
/// assert_eq!(extract_relation_prefix("owner_id", "user", "id"), "owner");
/// ```
pub fn extract_relation_prefix(fk_column: &str, current_table: &str, ref_column: &str) -> String {
    // Build the suffix to strip: _{ref_column} (e.g., "_id", "_idx")
    let ref_suffix = format!("_{}", ref_column);

    // Remove the ref_column suffix if present
    let without_ref = if fk_column.ends_with(&ref_suffix) {
        &fk_column[..fk_column.len() - ref_suffix.len()]
    } else {
        fk_column
    };

    let current_lower = current_table.to_lowercase();
    let without_ref_lower = without_ref.to_lowercase();

    // Case 1: FK column exactly matches current table (e.g., "user_id" for table "user")
    // This is the "default" FK - return empty prefix
    if without_ref_lower == current_lower {
        return String::new();
    }

    // Case 2: FK column ends with _{current_table} (e.g., "answered_by_user_id" for table "user")
    // Strip the _{table} suffix to get the semantic prefix
    let table_suffix = format!("_{}", current_lower);
    if without_ref_lower.ends_with(&table_suffix) {
        let prefix_len = without_ref.len() - table_suffix.len();
        return without_ref[..prefix_len].to_string();
    }

    // Case 3: FK column is a different role (e.g., "author_id" for table "user")
    // Use the column name as the prefix
    without_ref.to_string()
}

/// Generate reverse relation field name for has_many/has_one relations.
///
/// # Arguments
/// * `fk_columns` - The FK column names
/// * `current_table` - The table being referenced (e.g., "user")
/// * `source_table` - The table that has the FK (e.g., "inquiry")
/// * `ref_column` - The referenced column name (e.g., "id")
/// * `has_multiple_fks` - Whether source_table has multiple FKs to current_table
/// * `is_one_to_one` - Whether this is a has_one relation
///
/// # Returns
/// The field name (e.g., "inquiries", "answered_by_inquiries")
pub fn build_reverse_relation_field_name(
    fk_columns: &[String],
    current_table: &str,
    source_table: &str,
    ref_column: &str,
    has_multiple_fks: bool,
    is_one_to_one: bool,
) -> String {
    let base_name = if is_one_to_one {
        source_table.to_string()
    } else {
        pluralize(source_table)
    };

    if !has_multiple_fks || fk_columns.is_empty() {
        return base_name;
    }

    let prefix = extract_relation_prefix(&fk_columns[0], current_table, ref_column);

    if prefix.is_empty() {
        base_name
    } else {
        format!("{}_{}", prefix, base_name)
    }
}

/// Generate relation enum name for FK relations.
///
/// Uses the same logic as field naming but converts to PascalCase.
/// This ensures relation_enum aligns with field names for consistency.
///
/// # Examples
/// ```
/// use vespertide_naming::build_relation_enum_name;
///
/// assert_eq!(build_relation_enum_name(&["user_id".into()], "user", "id"), "");
/// assert_eq!(build_relation_enum_name(&["answered_by_user_id".into()], "user", "id"), "AnsweredBy");
/// assert_eq!(build_relation_enum_name(&["author_id".into()], "user", "id"), "Author");
/// ```
pub fn build_relation_enum_name(
    fk_columns: &[String],
    current_table: &str,
    ref_column: &str,
) -> String {
    if fk_columns.is_empty() {
        return String::new();
    }

    let prefix = extract_relation_prefix(&fk_columns[0], current_table, ref_column);

    if prefix.is_empty() {
        String::new()
    } else {
        to_pascal_case(&prefix)
    }
}

/// Convert snake_case to PascalCase.
///
/// # Examples
/// ```
/// use vespertide_naming::to_pascal_case;
///
/// assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
/// assert_eq!(to_pascal_case("answered_by"), "AnsweredBy");
/// assert_eq!(to_pascal_case("user"), "User");
/// ```
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for c in s.chars() {
        let is_separator = c == '_' || c == '-';
        if is_separator {
            capitalize = true;
            continue;
        }
        let ch = if capitalize {
            c.to_ascii_uppercase()
        } else {
            c
        };
        capitalize = false;
        result.push(ch);
    }
    result
}

/// Simple pluralization for relation field names.
///
/// # Examples
/// ```
/// use vespertide_naming::pluralize;
///
/// assert_eq!(pluralize("inquiry"), "inquiries");
/// assert_eq!(pluralize("comment"), "comments");
/// assert_eq!(pluralize("status"), "status");
/// ```
pub fn pluralize(name: &str) -> String {
    if name.ends_with('s') || name.ends_with("es") {
        name.to_string()
    } else if name.ends_with('y')
        && !name.ends_with("ay")
        && !name.ends_with("ey")
        && !name.ends_with("oy")
        && !name.ends_with("uy")
    {
        // e.g., category -> categories, inquiry -> inquiries
        format!("{}ies", &name[..name.len() - 1])
    } else {
        format!("{}s", name)
    }
}

// ============================================================================
// Constraint Naming (for SQL generation)
// ============================================================================

/// Generate index name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: ix_{table}__{key} or ix_{table}__{col1}_{col2}...
pub fn build_index_name(table: &str, columns: &[String], key: Option<&str>) -> String {
    match key {
        Some(k) => format!("ix_{}__{}", table, k),
        None => format!("ix_{}__{}", table, columns.join("_")),
    }
}

/// Generate unique constraint name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: uq_{table}__{key} or uq_{table}__{col1}_{col2}...
pub fn build_unique_constraint_name(table: &str, columns: &[String], key: Option<&str>) -> String {
    match key {
        Some(k) => format!("uq_{}__{}", table, k),
        None => format!("uq_{}__{}", table, columns.join("_")),
    }
}

/// Generate foreign key constraint name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: fk_{table}__{key} or fk_{table}__{col1}_{col2}...
pub fn build_foreign_key_name(table: &str, columns: &[String], key: Option<&str>) -> String {
    match key {
        Some(k) => format!("fk_{}__{}", table, k),
        None => format!("fk_{}__{}", table, columns.join("_")),
    }
}

/// Generate CHECK constraint name for SQLite enum column.
/// Uses double underscore to separate table name from the rest.
/// Format: chk_{table}__{column}
pub fn build_check_constraint_name(table: &str, column: &str) -> String {
    format!("chk_{}__{}", table, column)
}

/// Generate enum type name with table prefix to avoid conflicts.
/// Always includes table name to ensure uniqueness across tables.
/// Format: {table}_{enum_name}
///
/// This prevents conflicts when multiple tables use the same enum name
/// (e.g., "status" or "gender") with potentially different values.
pub fn build_enum_type_name(table: &str, enum_name: &str) -> String {
    format!("{}_{}", table, enum_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Relation Naming Tests
    // ========================================================================

    #[test]
    fn test_extract_relation_prefix_default_fk() {
        // Default FK: column matches table name + ref_column suffix
        assert_eq!(extract_relation_prefix("user_id", "user", "id"), "");
        assert_eq!(extract_relation_prefix("org_id", "org", "id"), "");
        assert_eq!(extract_relation_prefix("post_id", "post", "id"), "");
    }

    #[test]
    fn test_extract_relation_prefix_different_ref_column() {
        // Handle different ref_column suffixes (not just _id)
        assert_eq!(extract_relation_prefix("user_idx", "user", "idx"), "");
        assert_eq!(extract_relation_prefix("user_pk", "user", "pk"), "");
        assert_eq!(extract_relation_prefix("user_key", "user", "key"), "");
    }

    #[test]
    fn test_extract_relation_prefix_semantic_prefix() {
        // Prefixed FK: has semantic prefix before table name
        assert_eq!(
            extract_relation_prefix("answered_by_user_id", "user", "id"),
            "answered_by"
        );
        assert_eq!(
            extract_relation_prefix("created_by_user_id", "user", "id"),
            "created_by"
        );
        assert_eq!(
            extract_relation_prefix("target_user_id", "user", "id"),
            "target"
        );
        assert_eq!(
            extract_relation_prefix("parent_org_id", "org", "id"),
            "parent"
        );
    }

    #[test]
    fn test_extract_relation_prefix_role_fk() {
        // Role FK: column doesn't end with table name
        assert_eq!(extract_relation_prefix("author_id", "user", "id"), "author");
        assert_eq!(extract_relation_prefix("owner_id", "user", "id"), "owner");
        assert_eq!(
            extract_relation_prefix("creator_id", "user", "id"),
            "creator"
        );
    }

    #[test]
    fn test_extract_relation_prefix_no_suffix() {
        // Edge case: no ref_column suffix
        assert_eq!(extract_relation_prefix("user", "user", "id"), "");
        assert_eq!(extract_relation_prefix("admin_user", "user", "id"), "admin");
    }

    #[test]
    fn test_build_reverse_relation_field_name_single_fk() {
        // Single FK - just use source table name
        assert_eq!(
            build_reverse_relation_field_name(
                &["user_id".into()],
                "user",
                "inquiry",
                "id",
                false,
                false
            ),
            "inquiries"
        );
        assert_eq!(
            build_reverse_relation_field_name(
                &["author_id".into()],
                "user",
                "comment",
                "id",
                false,
                false
            ),
            "comments"
        );
    }

    #[test]
    fn test_build_reverse_relation_field_name_multiple_fks() {
        // Multiple FKs - need disambiguation
        assert_eq!(
            build_reverse_relation_field_name(
                &["user_id".into()],
                "user",
                "inquiry",
                "id",
                true,
                false
            ),
            "inquiries"
        );
        assert_eq!(
            build_reverse_relation_field_name(
                &["answered_by_user_id".into()],
                "user",
                "inquiry",
                "id",
                true,
                false
            ),
            "answered_by_inquiries"
        );
    }

    #[test]
    fn test_build_reverse_relation_field_name_one_to_one() {
        assert_eq!(
            build_reverse_relation_field_name(
                &["user_id".into()],
                "user",
                "profile",
                "id",
                false,
                true
            ),
            "profile"
        );
        assert_eq!(
            build_reverse_relation_field_name(
                &["backup_user_id".into()],
                "user",
                "settings",
                "id",
                true,
                true
            ),
            "backup_settings"
        );
    }

    #[test]
    fn test_build_relation_enum_name() {
        // Default FK - empty enum name (not needed or use table name)
        assert_eq!(
            build_relation_enum_name(&["user_id".into()], "user", "id"),
            ""
        );

        // Semantic prefix - PascalCase
        assert_eq!(
            build_relation_enum_name(&["answered_by_user_id".into()], "user", "id"),
            "AnsweredBy"
        );
        assert_eq!(
            build_relation_enum_name(&["target_user_id".into()], "user", "id"),
            "Target"
        );

        // Role FK - PascalCase of role
        assert_eq!(
            build_relation_enum_name(&["author_id".into()], "user", "id"),
            "Author"
        );
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("answered_by"), "AnsweredBy");
        assert_eq!(to_pascal_case("user"), "User");
        assert_eq!(to_pascal_case("hello-world"), "HelloWorld");
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize("inquiry"), "inquiries");
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("comment"), "comments");
        assert_eq!(pluralize("user"), "users");
        assert_eq!(pluralize("status"), "status");
        assert_eq!(pluralize("address"), "address");
    }

    // ========================================================================
    // Constraint Naming Tests
    // ========================================================================

    #[test]
    fn test_build_index_name_with_key() {
        assert_eq!(
            build_index_name("users", &["email".into()], Some("email_idx")),
            "ix_users__email_idx"
        );
    }

    #[test]
    fn test_build_index_name_without_key() {
        assert_eq!(
            build_index_name("users", &["email".into()], None),
            "ix_users__email"
        );
    }

    #[test]
    fn test_build_index_name_multiple_columns() {
        assert_eq!(
            build_index_name("users", &["first_name".into(), "last_name".into()], None),
            "ix_users__first_name_last_name"
        );
    }

    #[test]
    fn test_build_unique_constraint_name_with_key() {
        assert_eq!(
            build_unique_constraint_name("users", &["email".into()], Some("email_unique")),
            "uq_users__email_unique"
        );
    }

    #[test]
    fn test_build_unique_constraint_name_without_key() {
        assert_eq!(
            build_unique_constraint_name("users", &["email".into()], None),
            "uq_users__email"
        );
    }

    #[test]
    fn test_build_foreign_key_name_with_key() {
        assert_eq!(
            build_foreign_key_name("posts", &["user_id".into()], Some("fk_user")),
            "fk_posts__fk_user"
        );
    }

    #[test]
    fn test_build_foreign_key_name_without_key() {
        assert_eq!(
            build_foreign_key_name("posts", &["user_id".into()], None),
            "fk_posts__user_id"
        );
    }

    #[test]
    fn test_build_check_constraint_name() {
        assert_eq!(
            build_check_constraint_name("users", "status"),
            "chk_users__status"
        );
    }

    #[test]
    fn test_build_enum_type_name() {
        assert_eq!(build_enum_type_name("users", "status"), "users_status");
    }

    #[test]
    fn test_build_enum_type_name_with_existing_prefix() {
        // Even if enum_name already has table prefix, we add it
        // User should provide clean enum name (e.g., "status" not "users_status")
        assert_eq!(
            build_enum_type_name("users", "user_status"),
            "users_user_status"
        );
    }

    #[test]
    fn test_build_enum_type_name_prevents_conflicts() {
        // Different tables can have same enum name without conflict
        assert_eq!(build_enum_type_name("users", "gender"), "users_gender");
        assert_eq!(
            build_enum_type_name("employees", "gender"),
            "employees_gender"
        );

        assert_eq!(build_enum_type_name("orders", "status"), "orders_status");
        assert_eq!(
            build_enum_type_name("shipments", "status"),
            "shipments_status"
        );
    }
}
