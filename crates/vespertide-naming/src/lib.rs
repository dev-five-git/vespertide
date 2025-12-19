//! Naming conventions and helpers for vespertide database schema management.
//!
//! This crate provides consistent naming functions for database objects like
//! indexes, constraints, and foreign keys. It has no dependencies and can be
//! used by any other vespertide crate.

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
