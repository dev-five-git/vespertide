//! Wire format and DDL guard for [`MigrationAction::DataMigration`].
//!
//! [`MigrationAction::DataMigration`]: super::MigrationAction::DataMigration

use serde::{Deserialize, Serialize};

/// The SQL body of a [`MigrationAction::DataMigration`], either one portable
/// statement or one statement per backend.
///
/// The wire format is untagged, so both shapes are written naturally in a
/// migration file:
///
/// ```json
/// { "type": "data_migration", "sql": "UPDATE product SET price = 0 WHERE price IS NULL" }
/// ```
///
/// ```json
/// {
///   "type": "data_migration",
///   "sql": {
///     "postgres": "UPDATE t SET j = jsonb_build_object('ko', c)",
///     "mysql":    "UPDATE t SET j = JSON_OBJECT('ko', c)",
///     "sqlite":   "UPDATE t SET j = json_object('ko', c)"
///   }
/// }
/// ```
///
/// All three backend keys are required in the per-backend form. A missing key
/// would silently emit nothing for that backend — exactly the class of silent
/// data loss this action exists to prevent.
///
/// [`MigrationAction::DataMigration`]: super::MigrationAction::DataMigration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum DataMigrationSql {
    /// One portable statement executed verbatim on every backend.
    Uniform(String),
    /// One statement per backend, for data changes that cannot be expressed
    /// portably (JSON constructors, string functions, upsert syntax, …).
    PerBackend {
        /// Statement emitted for `PostgreSQL`.
        postgres: String,
        /// Statement emitted for `MySQL`.
        mysql: String,
        /// Statement emitted for `SQLite`.
        sqlite: String,
    },
}

impl DataMigrationSql {
    /// The statement emitted for `PostgreSQL`, verbatim.
    #[must_use]
    pub fn postgres(&self) -> &str {
        match self {
            Self::Uniform(sql) => sql,
            Self::PerBackend { postgres, .. } => postgres,
        }
    }

    /// The statement emitted for `MySQL`, verbatim.
    #[must_use]
    pub fn mysql(&self) -> &str {
        match self {
            Self::Uniform(sql) => sql,
            Self::PerBackend { mysql, .. } => mysql,
        }
    }

    /// The statement emitted for `SQLite`, verbatim.
    #[must_use]
    pub fn sqlite(&self) -> &str {
        match self {
            Self::Uniform(sql) => sql,
            Self::PerBackend { sqlite, .. } => sqlite,
        }
    }

    /// Every statement this value can emit, in backend order.
    ///
    /// The DDL guard checks *all* of them: a per-backend form whose `sqlite`
    /// branch smuggles in a `DROP TABLE` is just as fatal to baseline replay
    /// as a uniform one.
    pub fn statements(&self) -> impl Iterator<Item = &str> {
        match self {
            Self::Uniform(sql) => [Some(sql.as_str()), None, None],
            Self::PerBackend {
                postgres,
                mysql,
                sqlite,
            } => [
                Some(postgres.as_str()),
                Some(mysql.as_str()),
                Some(sqlite.as_str()),
            ],
        }
        .into_iter()
        .flatten()
    }
}

impl From<&str> for DataMigrationSql {
    fn from(sql: &str) -> Self {
        Self::Uniform(sql.to_string())
    }
}

impl From<String> for DataMigrationSql {
    fn from(sql: String) -> Self {
        Self::Uniform(sql)
    }
}

/// Statement keywords that change *schema* rather than *data*.
///
/// A `data_migration` starting with any of these breaks the action's
/// schema-neutrality contract, so it is rejected at load and plan time.
const DDL_KEYWORDS: [&str; 4] = ["CREATE", "ALTER", "DROP", "TRUNCATE"];

/// Strip leading whitespace and SQL comments (`-- line`, `/* block */`) so the
/// DDL guard sees the first real token.
///
/// Block comments are treated as non-nesting (ANSI SQL behaviour). A
/// deliberately nested comment can therefore hide a keyword from the guard;
/// that is a false *negative* in a pathological case, never a false positive.
fn strip_leading_trivia(sql: &str) -> &str {
    let mut rest = sql.trim_start();
    loop {
        rest = if let Some(after) = rest.strip_prefix("--") {
            after.split_once('\n').map_or("", |(_, tail)| tail)
        } else if let Some(after) = rest.strip_prefix("/*") {
            after.split_once("*/").map_or("", |(_, tail)| tail)
        } else {
            return rest;
        };
        rest = rest.trim_start();
    }
}

/// True when `body` opens with `keyword` as a complete token.
///
/// Matching is ASCII-case-insensitive and requires a token boundary after the
/// keyword, so `CREATED_AT` is not mistaken for `CREATE`.
fn starts_with_keyword(body: &str, keyword: &str) -> bool {
    let bytes = body.as_bytes();
    let keyword = keyword.as_bytes();
    if bytes.len() < keyword.len() {
        return false;
    }
    if !bytes[..keyword.len()].eq_ignore_ascii_case(keyword) {
        return false;
    }
    match bytes.get(keyword.len()) {
        None => true,
        Some(next) => !(next.is_ascii_alphanumeric() || *next == b'_'),
    }
}

/// Return the DDL keyword a statement opens with, if any.
///
/// Leading whitespace and comments are trimmed first, and the comparison is
/// case-insensitive, so `/* fix up */ drop table t` is caught just like
/// `DROP TABLE t`.
#[must_use]
pub fn leading_ddl_keyword(sql: &str) -> Option<&'static str> {
    let body = strip_leading_trivia(sql);
    DDL_KEYWORDS
        .into_iter()
        .find(|keyword| starts_with_keyword(body, keyword))
}

/// Bounded single-line preview of a SQL statement for error and warning text.
///
/// Leading trivia is dropped, internal whitespace runs collapse to one space,
/// and the result is truncated to 60 characters followed by `...`.
#[must_use]
pub fn sql_preview(sql: &str) -> String {
    let collapsed = strip_leading_trivia(sql)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.char_indices().nth(60).is_some() {
        let head: String = collapsed.chars().take(57).collect();
        format!("{head}...")
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn uniform_sql_wire_format_is_a_bare_string() {
        let canonical = r#""UPDATE user SET active = true""#;
        let parsed: DataMigrationSql = serde_json::from_str(canonical).expect("parse");
        assert_eq!(
            parsed,
            DataMigrationSql::Uniform("UPDATE user SET active = true".to_string())
        );
        assert_eq!(
            serde_json::to_string(&parsed).expect("serialize"),
            canonical
        );
    }

    #[test]
    fn per_backend_sql_wire_format_is_a_three_key_object() {
        let canonical = r#"{"postgres":"UPDATE a","mysql":"UPDATE b","sqlite":"UPDATE c"}"#;
        let parsed: DataMigrationSql = serde_json::from_str(canonical).expect("parse");
        assert_eq!(
            parsed,
            DataMigrationSql::PerBackend {
                postgres: "UPDATE a".to_string(),
                mysql: "UPDATE b".to_string(),
                sqlite: "UPDATE c".to_string(),
            }
        );
        assert_eq!(
            serde_json::to_string(&parsed).expect("serialize"),
            canonical
        );
    }

    #[test]
    fn per_backend_sql_requires_every_backend_key() {
        let missing_sqlite = r#"{"postgres":"UPDATE a","mysql":"UPDATE b"}"#;
        let parsed: Result<DataMigrationSql, _> = serde_json::from_str(missing_sqlite);
        assert!(
            parsed.is_err(),
            "a per-backend form missing a key must not deserialize: {parsed:?}"
        );
    }

    #[test]
    fn uniform_sql_is_returned_for_every_backend() {
        let sql = DataMigrationSql::from("UPDATE user SET x = 1");
        assert_eq!(sql.postgres(), "UPDATE user SET x = 1");
        assert_eq!(sql.mysql(), "UPDATE user SET x = 1");
        assert_eq!(sql.sqlite(), "UPDATE user SET x = 1");
        assert_eq!(
            sql.statements().collect::<Vec<_>>(),
            ["UPDATE user SET x = 1"]
        );
    }

    #[test]
    fn per_backend_sql_routes_each_backend_to_its_own_statement() {
        let sql = DataMigrationSql::PerBackend {
            postgres: "UPDATE pg".to_string(),
            mysql: "UPDATE my".to_string(),
            sqlite: "UPDATE lite".to_string(),
        };
        assert_eq!(sql.postgres(), "UPDATE pg");
        assert_eq!(sql.mysql(), "UPDATE my");
        assert_eq!(sql.sqlite(), "UPDATE lite");
        assert_eq!(
            sql.statements().collect::<Vec<_>>(),
            ["UPDATE pg", "UPDATE my", "UPDATE lite"]
        );
    }

    #[test]
    fn from_owned_string_builds_the_uniform_form() {
        let sql = DataMigrationSql::from("UPDATE user SET x = 1".to_string());
        assert_eq!(
            sql,
            DataMigrationSql::Uniform("UPDATE user SET x = 1".to_string())
        );
    }

    #[rstest]
    #[case::create("CREATE TABLE t (id int)", Some("CREATE"))]
    #[case::alter("ALTER TABLE t ADD COLUMN c int", Some("ALTER"))]
    #[case::drop("DROP TABLE t", Some("DROP"))]
    #[case::truncate("TRUNCATE TABLE t", Some("TRUNCATE"))]
    #[case::lowercase("drop table t", Some("DROP"))]
    #[case::mixed_case("CrEaTe TABLE t (id int)", Some("CREATE"))]
    #[case::leading_whitespace("\n\t  DROP TABLE t", Some("DROP"))]
    #[case::line_comment("-- clean up\nDROP TABLE t", Some("DROP"))]
    #[case::block_comment("/* clean up */ DROP TABLE t", Some("DROP"))]
    #[case::stacked_comments("-- one\n/* two */\n-- three\nALTER TABLE t", Some("ALTER"))]
    #[case::unterminated_line_comment("-- only a comment", None)]
    #[case::unterminated_block_comment("/* never closed", None)]
    #[case::update("UPDATE user SET active = true", None)]
    #[case::insert("INSERT INTO audit SELECT * FROM user", None)]
    #[case::delete("DELETE FROM session WHERE expired", None)]
    #[case::with_cte("WITH d AS (SELECT 1) UPDATE t SET x = 1", None)]
    #[case::ddl_word_inside_body("UPDATE t SET note = 'DROP TABLE'", None)]
    #[case::identifier_prefix("CREATED_AT_FIXUP()", None)]
    #[case::empty("", None)]
    #[case::whitespace_only("   \n  ", None)]
    fn leading_ddl_keyword_classifies_statements(
        #[case] sql: &str,
        #[case] expected: Option<&'static str>,
    ) {
        assert_eq!(leading_ddl_keyword(sql), expected);
    }

    #[test]
    fn ddl_keyword_needs_a_token_boundary_not_just_a_prefix() {
        // `DROPLET` starts with `DROP` but is a different token.
        assert_eq!(leading_ddl_keyword("DROPLET the_table"), None);
        // A shorter body than the keyword must not index out of bounds.
        assert_eq!(leading_ddl_keyword("DRO"), None);
        // End-of-input immediately after the keyword is a valid boundary.
        assert_eq!(leading_ddl_keyword("DROP"), Some("DROP"));
        // A non-ASCII byte right after the keyword still counts as a boundary.
        assert_eq!(leading_ddl_keyword("DROP\u{ad6d}"), Some("DROP"));
    }

    #[rstest]
    #[case::short("UPDATE t SET x = 1", "UPDATE t SET x = 1")]
    #[case::collapses_newlines("UPDATE t\n  SET x = 1", "UPDATE t SET x = 1")]
    #[case::strips_leading_comment("-- why\nUPDATE t SET x = 1", "UPDATE t SET x = 1")]
    fn sql_preview_normalises_short_statements(#[case] sql: &str, #[case] expected: &str) {
        assert_eq!(sql_preview(sql), expected);
    }

    #[test]
    fn sql_preview_truncates_at_the_60_character_boundary() {
        let exactly_60 = "0123456789".repeat(6);
        assert_eq!(sql_preview(&exactly_60), exactly_60);

        let sixty_one = format!("{exactly_60}X");
        let head: String = sixty_one.chars().take(57).collect();
        assert_eq!(sql_preview(&sixty_one), format!("{head}..."));
    }

    #[test]
    fn sql_preview_counts_characters_not_bytes() {
        let multibyte = "한".repeat(61);
        let head: String = multibyte.chars().take(57).collect();
        assert_eq!(sql_preview(&multibyte), format!("{head}..."));
    }
}
