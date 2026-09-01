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
/// * `fk_column` - The FK column name (e.g., "`user_id`", "`answered_by_user_id`", "`author_id`")
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
    let ref_suffix = format!("_{ref_column}");

    // Remove the ref_column suffix if present
    let without_ref = if let Some(stripped) = fk_column.strip_suffix(&ref_suffix) {
        stripped
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
    let table_suffix = format!("_{current_lower}");
    if without_ref_lower.ends_with(&table_suffix) {
        let prefix_chars = without_ref_lower.chars().count() - table_suffix.chars().count();
        return without_ref.chars().take(prefix_chars).collect();
    }

    // Case 3: FK column is a different role (e.g., "author_id" for table "user")
    // Use the column name as the prefix
    without_ref.to_string()
}

/// Generate reverse relation field name for `has_many/has_one` relations.
///
/// # Arguments
/// * `fk_columns` - The FK column names
/// * `current_table` - The table being referenced (e.g., "user")
/// * `source_table` - The table that has the FK (e.g., "inquiry")
/// * `ref_column` - The referenced column name (e.g., "id")
/// * `has_multiple_fks` - Whether `source_table` has multiple FKs to `current_table`
/// * `is_one_to_one` - Whether this is a `has_one` relation
///
/// # Returns
/// The field name (e.g., "inquiries", "`answered_by_inquiries`")
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
        format!("{prefix}_{base_name}")
    }
}

/// Generate relation enum name for FK relations.
///
/// Uses the same logic as field naming but converts to `PascalCase`.
/// This ensures `relation_enum` aligns with field names for consistency.
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

/// Convert `snake_case` to `PascalCase`.
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

/// Convert an arbitrary schema value into `SCREAMING_SNAKE_CASE`.
///
/// Word boundaries are detected on the lower→upper transition rather than on
/// character position, so a value that is already `SCREAMING_SNAKE_CASE`
/// survives unchanged. Non-alphanumeric characters become separators and
/// trailing separators are trimmed.
///
/// This is case conversion only — the result can still be an invalid identifier
/// (a value such as `1critical` keeps its leading digit). Pass it through
/// [`sanitize_identifier`] with the target language's rule before emitting it.
///
/// # Examples
/// ```
/// use vespertide_naming::to_screaming_snake_case;
///
/// assert_eq!(to_screaming_snake_case("inProgress"), "IN_PROGRESS");
/// assert_eq!(to_screaming_snake_case("order-status"), "ORDER_STATUS");
/// assert_eq!(to_screaming_snake_case("ERROR_LEVEL"), "ERROR_LEVEL");
/// assert_eq!(to_screaming_snake_case("1critical"), "1CRITICAL");
/// ```
pub fn to_screaming_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_lower = false;
    for ch in s.chars() {
        if ch.is_uppercase() && prev_lower {
            result.push('_');
        }
        if ch.is_alphanumeric() {
            result.push(ch.to_ascii_uppercase());
            prev_lower = ch.is_lowercase();
        } else {
            result.push('_');
            prev_lower = false;
        }
    }
    result.trim_end_matches('_').to_string()
}

/// What a target language accepts as the first character of an identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifierStart {
    /// Rust modules, Java, SQLAlchemy's Python models and ERD node ids all
    /// accept a leading `_`.
    Underscore,
    /// Prisma's schema parser and Pydantic (SQLModel fields) reject a leading
    /// `_` outright, and `SeaORM`'s `DeriveEntityModel` drops it while building
    /// the `Column` variant name. All three need a letter instead.
    Letter,
}

/// Rewrite `name` into an identifier the target language will accept.
///
/// Characters that cannot appear in an identifier become `_`, and a name that
/// would otherwise begin with a digit gains a prefix chosen by `start`. The
/// letter prefix follows the case of the name it precedes, so it reads the same
/// as its surroundings in both `PascalCase` and `snake_case` output.
///
/// Renaming loses the database name, so a caller that gets back something other
/// than what it passed in **must** emit the original alongside it — Prisma
/// `@map`, `SeaORM` `column_name`, SQLAlchemy's positional column name.
///
/// # Examples
/// ```
/// use vespertide_naming::{IdentifierStart, sanitize_identifier};
///
/// assert_eq!(sanitize_identifier("user-id", IdentifierStart::Underscore), "user_id");
/// assert_eq!(sanitize_identifier("1st_place", IdentifierStart::Underscore), "_1st_place");
/// assert_eq!(sanitize_identifier("1st_place", IdentifierStart::Letter), "x1st_place");
/// assert_eq!(sanitize_identifier("1CRITICAL", IdentifierStart::Letter), "X1CRITICAL");
/// assert_eq!(sanitize_identifier("email", IdentifierStart::Letter), "email");
/// ```
pub fn sanitize_identifier(name: &str, start: IdentifierStart) -> String {
    let mut result: String = name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();

    let needs_prefix = match start {
        IdentifierStart::Underscore => {
            result.is_empty() || result.starts_with(|c: char| c.is_ascii_digit())
        }
        IdentifierStart::Letter => !result.starts_with(|c: char| c.is_ascii_alphabetic()),
    };
    if !needs_prefix {
        return result;
    }

    let prefix = match start {
        IdentifierStart::Underscore => '_',
        // Match the case of the name's first letter so the escape blends into
        // `PascalCase`, `snake_case` and `SCREAMING_SNAKE_CASE` alike.
        IdentifierStart::Letter => {
            if result
                .chars()
                .find(char::is_ascii_alphabetic)
                .is_some_and(char::is_uppercase)
            {
                'X'
            } else {
                'x'
            }
        }
    };
    result.insert(0, prefix);
    result
}

/// Module name for a `SeaORM` entity file.
///
/// The exporter writes `super::{module}::Entity` in relation fields while the
/// CLI writes `pub mod {module};` and names the file `{module}.rs`. All three
/// have to agree, so the rule lives here rather than in either caller.
///
/// Rust would accept a leading `_`, but `sea-orm` infers the `Relation` variant
/// of a relation field from the target's module name by `PascalCase`-ing it —
/// which drops the `_` and leaves `1users`, so the derive panics. Hence the
/// same letter rule the entity's own fields use.
///
/// # Examples
/// ```
/// use vespertide_naming::seaorm_module_name;
///
/// assert_eq!(seaorm_module_name("users"), "users");
/// assert_eq!(seaorm_module_name("1users"), "x1users");
/// assert_eq!(seaorm_module_name("user-profile"), "user_profile");
/// ```
pub fn seaorm_module_name(name: &str) -> String {
    sanitize_identifier(name, IdentifierStart::Letter)
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
        format!("{}ies", name.strip_suffix('y').unwrap_or(name))
    } else {
        format!("{name}s")
    }
}

// ============================================================================
// Constraint Naming (for SQL generation)
// ============================================================================

/// Generate index name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: ix_{table}__{key} or ix_{table}__{col1}_{col2}...
pub fn build_index_name<T: AsRef<str>>(table: &str, columns: &[T], key: Option<&str>) -> String {
    build_constraint_name("ix_", table, columns, key)
}

/// Generate unique constraint name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: uq_{table}__{key} or uq_{table}__{col1}_{col2}...
pub fn build_unique_constraint_name<T: AsRef<str>>(
    table: &str,
    columns: &[T],
    key: Option<&str>,
) -> String {
    build_constraint_name("uq_", table, columns, key)
}

/// Generate foreign key constraint name from table name, columns, and optional user-provided key.
/// Always includes table name to avoid conflicts across tables.
/// Uses double underscore to separate table name from the rest.
/// Format: fk_{table}__{key} or fk_{table}__{col1}_{col2}...
pub fn build_foreign_key_name<T: AsRef<str>>(
    table: &str,
    columns: &[T],
    key: Option<&str>,
) -> String {
    build_constraint_name("fk_", table, columns, key)
}

/// Shared body for the three constraint name builders above.
///
/// Folds the `{prefix}{table}__{key|sorted-columns}` template into a single
/// pre-sized `String` so the auto-named branch ( `key.is_none()`) does only
/// two allocations: the column-sort scratchpad (`Vec<&str>`) and the final
/// `String`. The previous implementation went through `format!(... join("_"))`
/// which allocated an extra intermediate `String` for the joined columns
/// before formatting them into the final result.
fn build_constraint_name<T: AsRef<str>>(
    prefix: &str,
    table: &str,
    columns: &[T],
    key: Option<&str>,
) -> String {
    if let Some(k) = key {
        let mut out = String::with_capacity(prefix.len() + table.len() + 2 + k.len());
        out.push_str(prefix);
        out.push_str(table);
        out.push_str("__");
        out.push_str(k);
        out
    } else {
        let cols_capacity: usize = columns
            .iter()
            .map(|c| c.as_ref().len() + 1)
            .sum::<usize>()
            .saturating_sub(1);
        let mut out = String::with_capacity(prefix.len() + table.len() + 2 + cols_capacity);
        out.push_str(prefix);
        out.push_str(table);
        out.push_str("__");
        write_sorted_columns(&mut out, columns);
        out
    }
}

/// Sort the column slice into a local scratchpad and write the columns into
/// `out` joined by `'_'`. Replaces the previous `sort_columns_for_name(...).join("_")`
/// pair which allocated a fresh `String` for the joined columns; here the
/// columns go directly into the caller-supplied buffer.
fn write_sorted_columns<T: AsRef<str>>(out: &mut String, columns: &[T]) {
    let mut sorted: Vec<&str> = columns.iter().map(AsRef::as_ref).collect();
    sorted.sort_unstable();
    for (i, c) in sorted.iter().enumerate() {
        if i > 0 {
            out.push('_');
        }
        out.push_str(c);
    }
}

/// Generate CHECK constraint name for `SQLite` enum column.
/// Uses double underscore to separate table name from the rest.
/// Format: chk_{table}__{column}
pub fn build_check_constraint_name(table: &str, column: &str) -> String {
    // Build in one exact-size allocation like the sibling constraint-name
    // builders above, instead of routing through `format!`. Output is
    // byte-identical: `chk_{table}__{column}` (`"chk_"` = 4 bytes, `"__"` = 2).
    let mut out = String::with_capacity(4 + table.len() + 2 + column.len());
    out.push_str("chk_");
    out.push_str(table);
    out.push_str("__");
    out.push_str(column);
    out
}

/// Generate enum type name with table prefix to avoid conflicts.
/// Always includes table name to ensure uniqueness across tables.
/// Format: {table}_{`enum_name`}
///
/// This prevents conflicts when multiple tables use the same enum name
/// (e.g., "status" or "gender") with potentially different values.
pub fn build_enum_type_name(table: &str, enum_name: &str) -> String {
    // Build in one exact-size allocation like the sibling constraint-name
    // builders above, instead of routing through `format!`. Output is
    // byte-identical: `{table}_{enum_name}` (`'_'` = 1 byte).
    let mut out = String::with_capacity(table.len() + 1 + enum_name.len());
    out.push_str(table);
    out.push('_');
    out.push_str(enum_name);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rstest::rstest;

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
    fn test_extract_relation_prefix_unicode_table_name() {
        assert_eq!(
            extract_relation_prefix("작성자_한국어테이블_id", "한국어테이블", "id"),
            "작성자"
        );
        assert_eq!(extract_relation_prefix("📊_stats_id", "📊_stats", "id"), "");
    }

    #[test]
    fn test_pluralize_unicode_name_ending_with_ascii_y() {
        assert_eq!(pluralize("café_category"), "café_categories");
    }

    proptest! {
        #[test]
        fn extract_relation_prefix_does_not_panic_on_unicode(
            table in proptest::collection::vec(any::<char>(), 0..30).prop_map(|v| v.into_iter().collect::<String>()),
            prefix in proptest::collection::vec(any::<char>(), 0..30).prop_map(|v| v.into_iter().collect::<String>())
        ) {
            let fk_column = format!("{prefix}_{table}_id");
            let _ = extract_relation_prefix(&fk_column, &table, "id");
        }
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
        // Empty fk_columns - early return
        assert_eq!(build_relation_enum_name(&[], "user", "id"), "");

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

    #[rstest]
    #[case::lowercase("pending", "PENDING")]
    #[case::snake_case("not_started", "NOT_STARTED")]
    #[case::camel_case("inProgress", "IN_PROGRESS")]
    #[case::kebab_case("order-status", "ORDER_STATUS")]
    #[case::empty("", "")]
    // Already-uppercase input survives: a position-based word-boundary rule
    // would explode these into `E_R_R_O_R__L_E_V_E_L` / `H_T_T_P__500`.
    #[case::already_screaming("ERROR_LEVEL", "ERROR_LEVEL")]
    #[case::acronym_with_digits("HTTP_500", "HTTP_500")]
    #[case::trailing_separator("status-", "STATUS")]
    // Case conversion only: making this a valid identifier is
    // `sanitize_identifier`'s job, since the rule differs per language.
    #[case::leading_digit("1critical", "1CRITICAL")]
    fn test_to_screaming_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_screaming_snake_case(input), expected);
    }

    #[rstest]
    #[case::already_valid("email", IdentifierStart::Underscore, "email")]
    #[case::internal_digits("table_99_ok", IdentifierStart::Underscore, "table_99_ok")]
    #[case::hyphen("user-id", IdentifierStart::Underscore, "user_id")]
    #[case::space("user id", IdentifierStart::Underscore, "user_id")]
    #[case::non_ascii("사용자", IdentifierStart::Underscore, "___")]
    #[case::leading_digit_underscore("1st_place", IdentifierStart::Underscore, "_1st_place")]
    #[case::leading_digit_letter("1st_place", IdentifierStart::Letter, "x1st_place")]
    // The escape follows the case of the first letter it precedes.
    #[case::leading_digit_screaming("1CRITICAL", IdentifierStart::Letter, "X1CRITICAL")]
    // Prisma and Pydantic reject a leading `_`, so it is escaped too.
    #[case::existing_underscore_letter("_private", IdentifierStart::Letter, "x_private")]
    #[case::existing_underscore_ok("_private", IdentifierStart::Underscore, "_private")]
    #[case::empty_underscore("", IdentifierStart::Underscore, "_")]
    #[case::empty_letter("", IdentifierStart::Letter, "x")]
    fn test_sanitize_identifier(
        #[case] input: &str,
        #[case] start: IdentifierStart,
        #[case] expected: &str,
    ) {
        assert_eq!(sanitize_identifier(input, start), expected);
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

    /// The three builders monomorphized for `&[&str]` columns.
    /// Column strings are `'static` so the generic builders (whose column
    /// lifetime is fixed at instantiation) can coerce to a single fn pointer.
    type BuildFn = fn(&str, &[&'static str], Option<&str>) -> String;
    /// The three builders monomorphized for `&[String]` columns.
    type BuildStringFn = fn(&str, &[String], Option<&str>) -> String;

    #[rstest]
    #[case::index_with_key(build_index_name, "users", &["email"][..], Some("email_idx"), "ix_users__email_idx")]
    #[case::index_without_key(build_index_name, "users", &["email"][..], None, "ix_users__email")]
    #[case::index_multiple_columns(build_index_name, "users", &["first_name", "last_name"][..], None, "ix_users__first_name_last_name")]
    #[case::unique_with_key(build_unique_constraint_name, "users", &["email"][..], Some("email_unique"), "uq_users__email_unique")]
    #[case::unique_without_key(build_unique_constraint_name, "users", &["email"][..], None, "uq_users__email")]
    #[case::foreign_key_with_key(build_foreign_key_name, "posts", &["user_id"][..], Some("fk_user"), "fk_posts__fk_user")]
    #[case::foreign_key_without_key(build_foreign_key_name, "posts", &["user_id"][..], None, "fk_posts__user_id")]
    fn constraint_name_builders_render_expected(
        #[case] build: BuildFn,
        #[case] table: &str,
        #[case] columns: &[&'static str],
        #[case] key: Option<&str>,
        #[case] expected: &str,
    ) {
        assert_eq!(build(table, columns, key), expected);
    }

    /// Column order must not affect the generated name (`&[&str]` instantiation).
    #[rstest]
    #[case::index(build_index_name, "users", &["last_name", "first_name"][..])]
    #[case::unique(build_unique_constraint_name, "users", &["last_name", "first_name"][..])]
    #[case::foreign_key(build_foreign_key_name, "posts", &["tenant_id", "user_id"][..])]
    fn constraint_name_builders_sort_str_columns_for_deterministic_name(
        #[case] build: BuildFn,
        #[case] table: &str,
        #[case] columns: &[&'static str],
    ) {
        let mut reversed = columns.to_vec();
        reversed.reverse();
        assert_eq!(build(table, columns, None), build(table, &reversed, None));
    }

    /// Column order must not affect the generated name (`&[String]` instantiation).
    #[rstest]
    #[case::index(build_index_name, "users", &["last_name", "first_name"][..])]
    #[case::unique(build_unique_constraint_name, "order_items", &["product_id", "order_id"][..])]
    #[case::foreign_key(build_foreign_key_name, "memberships", &["tenant_id", "account_id"][..])]
    fn constraint_name_builders_sort_string_columns_for_deterministic_name(
        #[case] build: BuildStringFn,
        #[case] table: &str,
        #[case] columns: &[&str],
    ) {
        let columns: Vec<String> = columns.iter().map(ToString::to_string).collect();
        let mut reversed = columns.clone();
        reversed.reverse();
        assert_eq!(build(table, &columns, None), build(table, &reversed, None));
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
