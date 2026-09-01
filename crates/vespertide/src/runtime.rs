use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
};

use crate::MigrationError;

fn database_error(message: String, source: sea_orm::DbErr) -> MigrationError {
    MigrationError::Database {
        message,
        source: Some(Box::new(source)),
    }
}

/// Compiled migration with per-backend SQL byte arrays.
/// A single migration baked into the binary at compile time by the `vespertide_migration!` macro.
///
/// Each `EmbeddedMigration` holds three pre-compiled SQL blobs (one per supported backend).
/// Individual SQL statements within a blob are separated by null bytes (`\0`); use
/// [`split_sql_blob`] to iterate over them.
///
/// You do not construct `EmbeddedMigration` values manually. The `vespertide_migration!` macro
/// generates a `const` array of these and passes it to [`run_embedded_migrations`].
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedMigration {
    /// Monotonically increasing version number matching the migration file.
    pub version: u32,
    /// UUID identifying this specific migration plan, used to detect history divergence.
    pub migration_id: &'static str,
    /// Human-readable description of what this migration does.
    pub comment: &'static str,
    /// Null-byte-delimited SQL statements for `PostgreSQL`.
    pub postgres_sql_blob: &'static str,
    /// Null-byte-delimited SQL statements for `MySQL`.
    pub mysql_sql_blob: &'static str,
    /// Null-byte-delimited SQL statements for `SQLite`.
    pub sqlite_sql_blob: &'static str,
}

impl EmbeddedMigration {
    /// Construct an embedded migration; called by the `vespertide_migration!` macro expansion. Not intended for hand-written use.
    pub const fn new(
        version: u32,
        migration_id: &'static str,
        comment: &'static str,
        postgres_sql_blob: &'static str,
        mysql_sql_blob: &'static str,
        sqlite_sql_blob: &'static str,
    ) -> Self {
        Self {
            version,
            migration_id,
            comment,
            postgres_sql_blob,
            mysql_sql_blob,
            sqlite_sql_blob,
        }
    }

    pub const fn sql_blob(self, backend: DatabaseBackend) -> &'static str {
        match backend {
            DatabaseBackend::MySql => self.mysql_sql_blob,
            DatabaseBackend::Sqlite => self.sqlite_sql_blob,
            // Postgres today, plus any future `#[non_exhaustive]` backend
            // variants — preserves the historical Postgres fallback.
            _ => self.postgres_sql_blob,
        }
    }
}

pub fn split_sql_blob(blob: &str) -> impl Iterator<Item = &str> {
    blob.split_terminator('\0').filter(|sql| !sql.is_empty())
}

/// Runtime knobs for [`run_embedded_migrations_with_options`] (fault F94).
///
/// Both timeouts are optional and expressed in **milliseconds**. `None`
/// leaves the backend default untouched. The values are rendered to a
/// backend-appropriate statement injected at the start of the migration
/// session so a migration that blocks on a lock (or runs away) fails fast
/// instead of hanging the database.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MigrationRuntimeOptions {
    /// Max time (ms) to wait acquiring a lock before failing.
    /// `PostgreSQL` `lock_timeout`, `MySQL` `innodb_lock_wait_timeout`
    /// (rounded up to whole seconds), `SQLite` `PRAGMA busy_timeout`.
    pub lock_timeout_ms: Option<u64>,
    /// Max time (ms) a single statement may run. `PostgreSQL`
    /// `statement_timeout`, `MySQL` `max_execution_time`. `SQLite` has no
    /// statement timeout, so this is skipped on `SQLite`.
    pub statement_timeout_ms: Option<u64>,
}

impl MigrationRuntimeOptions {
    /// Construct from optional millisecond timeouts.
    ///
    /// This is the stable constructor used by `vespertide_migration!`
    /// macro-generated code: the struct is `#[non_exhaustive]`, so user
    /// crates cannot build it with a struct literal and must call this.
    #[must_use]
    pub const fn from_millis(
        lock_timeout_ms: Option<u64>,
        statement_timeout_ms: Option<u64>,
    ) -> Self {
        Self {
            lock_timeout_ms,
            statement_timeout_ms,
        }
    }

    /// True when neither timeout is configured (no SET statements emitted).
    fn is_noop(self) -> bool {
        self.lock_timeout_ms.is_none() && self.statement_timeout_ms.is_none()
    }
}

/// Render the connection-level (pre-transaction) timeout statements for a
/// backend. Only `SQLite` needs this (`PRAGMA busy_timeout` is connection
/// scoped and cannot run inside a transaction).
fn pre_txn_timeout_sql(backend: DatabaseBackend, options: MigrationRuntimeOptions) -> Vec<String> {
    let mut out = Vec::new();
    if matches!(backend, DatabaseBackend::Sqlite)
        && let Some(ms) = options.lock_timeout_ms
    {
        out.push(format!("PRAGMA busy_timeout = {ms}"));
    }
    out
}

/// Render the transaction-level timeout statements for a backend
/// (`PostgreSQL` `SET LOCAL`, `MySQL` `SET SESSION`). `SQLite`'s lock timeout
/// is handled pre-transaction; `SQLite` has no statement timeout.
fn in_txn_timeout_sql(backend: DatabaseBackend, options: MigrationRuntimeOptions) -> Vec<String> {
    let mut out = Vec::new();
    match backend {
        DatabaseBackend::Postgres => {
            if let Some(ms) = options.lock_timeout_ms {
                out.push(format!("SET LOCAL lock_timeout = {ms}"));
            }
            if let Some(ms) = options.statement_timeout_ms {
                out.push(format!("SET LOCAL statement_timeout = {ms}"));
            }
        }
        DatabaseBackend::MySql => {
            if let Some(ms) = options.lock_timeout_ms {
                // MySQL innodb_lock_wait_timeout is in SECONDS; round up so a
                // sub-second config never collapses to 0 (= "no wait").
                let secs = ms.div_ceil(1000).max(1);
                out.push(format!("SET SESSION innodb_lock_wait_timeout = {secs}"));
            }
            if let Some(ms) = options.statement_timeout_ms {
                out.push(format!("SET SESSION max_execution_time = {ms}"));
            }
        }
        // SQLite lock timeout handled pre-transaction; no statement timeout.
        // sea_orm::DatabaseBackend is #[non_exhaustive] → catch-all required.
        _ => {}
    }
    out
}

/// Irreducible shell: thin delegation to
/// [`run_embedded_migrations_with_options`] with default options. The body
/// is a single `await` and cannot be unit-tested without a live
/// [`DatabaseConnection`]; integration tests live in the out-of-workspace
/// `tests/runtime-sqlite` crate (excluded from the tarpaulin workspace due to
/// the `libsqlite3-sys` single-version `links = "sqlite3"` constraint).
#[cfg(not(tarpaulin_include))]
pub async fn run_embedded_migrations(
    pool: &DatabaseConnection,
    version_table: &str,
    verbose: bool,
    migrations: &[EmbeddedMigration],
) -> Result<(), MigrationError> {
    run_embedded_migrations_with_options(
        pool,
        version_table,
        verbose,
        migrations,
        MigrationRuntimeOptions::default(),
    )
    .await
}

/// Like [`run_embedded_migrations`] but applies the timeout knobs in
/// [`MigrationRuntimeOptions`] (fault F94) at the start of the migration
/// session. Backend-appropriate SET / PRAGMA statements are emitted so a
/// migration cannot hang indefinitely on a lock or a runaway statement.
///
/// [`run_embedded_migrations`] delegates here with default (no-timeout)
/// options, so existing callers are unaffected.
/// Irreducible shell: full migration flow against a live
/// [`DatabaseConnection`] (version-table setup, pre/in-txn timeout SETs,
/// version-read, apply loop, commit). Every observable step is `await`ed
/// against a real backend, so there is no workspace-safe unit-test path; the
/// pure helpers it composes (`pre_txn_timeout_sql`, `in_txn_timeout_sql`,
/// `collect_version_ids`, `split_sql_blob`, `database_error`,
/// `EmbeddedMigration::sql_blob`, `MigrationRuntimeOptions::is_noop`) are
/// each unit-tested in the inline `tests` module below. End-to-end coverage
/// lives in the out-of-workspace `tests/runtime-sqlite` crate (excluded from
/// the tarpaulin workspace due to the `libsqlite3-sys` single-version
/// `links = "sqlite3"` constraint).
#[cfg(not(tarpaulin_include))]
#[expect(
    clippy::print_stderr,
    reason = "verbose runtime migrations stream progress diagnostics to stderr while leaving host stdout application-owned"
)]
pub async fn run_embedded_migrations_with_options(
    pool: &DatabaseConnection,
    version_table: &str,
    verbose: bool,
    migrations: &[EmbeddedMigration],
    options: MigrationRuntimeOptions,
) -> Result<(), MigrationError> {
    let backend = pool.get_database_backend();
    let q = if matches!(backend, DatabaseBackend::MySql) {
        '`'
    } else {
        '"'
    };

    let create_table_sql = format!(
        "CREATE TABLE IF NOT EXISTS {q}{version_table}{q} (version INTEGER PRIMARY KEY, id TEXT DEFAULT '', created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)"
    );
    let stmt = Statement::from_string(backend, create_table_sql);
    pool.execute_raw(stmt)
        .await
        .map_err(|e| database_error(format!("Failed to create version table: {e}"), e))?;

    let alter_sql = format!("ALTER TABLE {q}{version_table}{q} ADD COLUMN id TEXT DEFAULT ''");
    let stmt = Statement::from_string(backend, alter_sql);
    let _ = pool.execute_raw(stmt).await;

    // F94: connection-level timeouts that cannot run inside a transaction
    // (SQLite `PRAGMA busy_timeout`) are applied here, before `begin()`.
    if !options.is_noop() {
        for sql in pre_txn_timeout_sql(backend, options) {
            if verbose {
                eprintln!("[vespertide] {sql}");
            }
            let stmt = Statement::from_string(backend, sql.clone());
            pool.execute_raw(stmt)
                .await
                .map_err(|e| database_error(format!("Failed to apply timeout '{sql}': {e}"), e))?;
        }
    }

    let txn = pool
        .begin()
        .await
        .map_err(|e| database_error(format!("Failed to begin transaction: {e}"), e))?;

    // F94: transaction-scoped timeouts (PostgreSQL `SET LOCAL`, MySQL
    // `SET SESSION`) are applied right after BEGIN, before any lock-taking
    // statement, so they cover the whole migration.
    if !options.is_noop() {
        for sql in in_txn_timeout_sql(backend, options) {
            if verbose {
                eprintln!("[vespertide] {sql}");
            }
            let stmt = Statement::from_string(backend, sql.clone());
            txn.execute_raw(stmt)
                .await
                .map_err(|e| database_error(format!("Failed to apply timeout '{sql}': {e}"), e))?;
        }
    }

    let select_sql = format!("SELECT MAX(version) as version FROM {q}{version_table}{q}");
    let stmt = Statement::from_string(backend, select_sql);
    let version_result = txn
        .query_one_raw(stmt)
        .await
        .map_err(|e| database_error(format!("Failed to read version: {e}"), e))?;
    let version_i32 = version_result
        .and_then(|row| row.try_get::<i32>("", "version").ok())
        .unwrap_or(0);
    // Migration versions are generated by Vespertide as non-negative u32 values;
    // treat a corrupt negative database value as no applied migration.
    let version = u32::try_from(version_i32).unwrap_or(0);

    let select_ids_sql = format!("SELECT version, id FROM {q}{version_table}{q}");
    let stmt = Statement::from_string(backend, select_ids_sql);
    let id_rows = txn
        .query_all_raw(stmt)
        .await
        .map_err(|e| database_error(format!("Failed to read version ids: {e}"), e))?;
    let version_ids = collect_version_ids(&id_rows);

    if verbose {
        eprintln!("[vespertide] Current database version: {version}");
    }

    for migration in migrations {
        if version >= migration.version {
            continue;
        }

        if let Some(db_id) = version_ids.get(&migration.version)
            && !migration.migration_id.is_empty()
            && !db_id.is_empty()
            && db_id != migration.migration_id
        {
            return Err(MigrationError::IdMismatch {
                version: migration.version,
                expected: migration.migration_id.to_string(),
                found: db_id.clone(),
            });
        }

        if verbose {
            eprintln!(
                "[vespertide] Applying migration v{} ({})",
                migration.version, migration.comment
            );
        }

        let sql_blob = migration.sql_blob(backend);
        let sqls: Vec<_> = split_sql_blob(sql_blob).collect();

        for (sql_idx, sql) in sqls.iter().enumerate() {
            if verbose {
                eprintln!("[vespertide]   [{}/{}] {}", sql_idx + 1, sqls.len(), sql);
            }

            let stmt = Statement::from_string(backend, (*sql).to_owned());
            txn.execute_raw(stmt)
                .await
                .map_err(|e| database_error(format!("Failed to execute SQL '{sql}': {e}"), e))?;
        }

        let insert_sql = format!(
            "INSERT INTO {q}{}{q} (version, id) VALUES ({}, '{}')",
            version_table, migration.version, migration.migration_id
        );
        let stmt = Statement::from_string(backend, insert_sql);
        txn.execute_raw(stmt)
            .await
            .map_err(|e| database_error(format!("Failed to insert version: {e}"), e))?;

        if verbose {
            eprintln!(
                "[vespertide] Migration v{} applied successfully",
                migration.version
            );
        }
    }

    txn.commit()
        .await
        .map_err(|e| database_error(format!("Failed to commit transaction: {e}"), e))?;

    Ok(())
}

fn collect_version_ids(rows: &[QueryResult]) -> std::collections::HashMap<u32, String> {
    let mut version_ids = std::collections::HashMap::new();
    for row in rows {
        if let Ok(found_version) = row.try_get::<i32>("", "version") {
            let id = row.try_get::<String>("", "id").unwrap_or_default();
            if let Ok(found_version) = u32::try_from(found_version) {
                version_ids.insert(found_version, id);
            }
        }
    }
    version_ids
}

// NOTE: tests that exercise `Database::connect("sqlite::memory:")` are kept
// in the workspace-excluded crate `tests/runtime-sqlite/`. Those need the
// sea-orm `sqlx-sqlite` feature, which links libsqlite3-sys 0.30 and
// conflicts on `links = "sqlite3"` with vespertide-query's rusqlite 0.39
// (libsqlite3-sys 0.37). Keeping them out of the workspace lets every crate
// stay on latest pinned versions.
//
// Run via: cargo test --manifest-path tests/runtime-sqlite/Cargo.toml
#[cfg(test)]
mod tests {
    use std::error::Error;

    use sea_orm::DatabaseBackend;

    use crate::MigrationError;

    use super::{
        EmbeddedMigration, MigrationRuntimeOptions, collect_version_ids, database_error,
        in_txn_timeout_sql, pre_txn_timeout_sql, split_sql_blob,
    };

    #[test]
    fn split_sql_blob_ignores_empty_segments() {
        let sqls: Vec<_> =
            split_sql_blob("CREATE TABLE users ();\0\0ALTER TABLE users;\0").collect();

        assert_eq!(sqls, vec!["CREATE TABLE users ();", "ALTER TABLE users;"]);
    }

    #[test]
    fn split_sql_blob_empty_input_yields_no_segments() {
        let sqls: Vec<_> = split_sql_blob("").collect();
        assert!(sqls.is_empty());
    }

    #[test]
    fn split_sql_blob_single_segment_no_terminator() {
        let sqls: Vec<_> = split_sql_blob("SELECT 1").collect();
        assert_eq!(sqls, vec!["SELECT 1"]);
    }

    #[test]
    fn embedded_migration_selects_backend_blob() {
        let migration = EmbeddedMigration::new(1, "id", "comment", "pg\0", "mysql\0", "sqlite\0");

        assert_eq!(migration.sql_blob(DatabaseBackend::Postgres), "pg\0");
        assert_eq!(migration.sql_blob(DatabaseBackend::MySql), "mysql\0");
        assert_eq!(migration.sql_blob(DatabaseBackend::Sqlite), "sqlite\0");
    }

    #[test]
    fn embedded_migration_new_records_all_fields() {
        let migration =
            EmbeddedMigration::new(42, "uuid-7", "create users", "pg-sql", "my-sql", "lite-sql");

        assert_eq!(migration.version, 42);
        assert_eq!(migration.migration_id, "uuid-7");
        assert_eq!(migration.comment, "create users");
        assert_eq!(migration.postgres_sql_blob, "pg-sql");
        assert_eq!(migration.mysql_sql_blob, "my-sql");
        assert_eq!(migration.sqlite_sql_blob, "lite-sql");

        // Clone + Copy semantics
        let migration_copy = migration;
        assert_eq!(migration_copy.version, migration.version);
    }

    #[test]
    fn db_err_conversion_preserves_source_chain() {
        let db_err = sea_orm::DbErr::Custom("connection refused".to_owned());
        let err = MigrationError::from(db_err);

        assert_eq!(
            err.to_string(),
            "database error: Custom Error: connection refused"
        );
        assert!(err.source().is_some());
        assert!(matches!(
            &err,
            MigrationError::Database {
                source: Some(_),
                ..
            }
        ));
        assert!(
            err.source()
                .is_some_and(|source| source.to_string() == "Custom Error: connection refused")
        );
    }

    #[test]
    fn database_error_wraps_message_and_chains_source() {
        let db_err = sea_orm::DbErr::Custom("nope".to_owned());
        let err = database_error("apply failed".to_owned(), db_err);

        match &err {
            MigrationError::Database { message, source } => {
                assert_eq!(message, "apply failed");
                let chained = source.as_ref().expect("source set by database_error");
                assert_eq!(chained.to_string(), "Custom Error: nope");
            }
            other => panic!("expected Database variant, got {other:?}"),
        }
        assert!(err.source().is_some());
    }

    // -- MigrationRuntimeOptions -------------------------------------------

    #[test]
    fn migration_runtime_options_default_is_noop() {
        let opts = MigrationRuntimeOptions::default();
        assert_eq!(opts.lock_timeout_ms, None);
        assert_eq!(opts.statement_timeout_ms, None);
        assert!(opts.is_noop());
    }

    #[test]
    fn migration_runtime_options_from_millis_records_both_timeouts() {
        let opts = MigrationRuntimeOptions::from_millis(Some(1000), Some(2000));
        assert_eq!(opts.lock_timeout_ms, Some(1000));
        assert_eq!(opts.statement_timeout_ms, Some(2000));
        assert!(!opts.is_noop());
    }

    #[test]
    fn migration_runtime_options_from_millis_lock_only_is_not_noop() {
        let opts = MigrationRuntimeOptions::from_millis(Some(500), None);
        assert_eq!(opts.lock_timeout_ms, Some(500));
        assert_eq!(opts.statement_timeout_ms, None);
        assert!(!opts.is_noop());
    }

    #[test]
    fn migration_runtime_options_from_millis_statement_only_is_not_noop() {
        let opts = MigrationRuntimeOptions::from_millis(None, Some(3000));
        assert_eq!(opts.lock_timeout_ms, None);
        assert_eq!(opts.statement_timeout_ms, Some(3000));
        assert!(!opts.is_noop());
    }

    #[test]
    fn migration_runtime_options_from_millis_no_timeouts_is_noop() {
        let opts = MigrationRuntimeOptions::from_millis(None, None);
        assert!(opts.is_noop());
        assert_eq!(opts, MigrationRuntimeOptions::default());
    }

    #[test]
    fn migration_runtime_options_supports_copy_clone_eq() {
        let a = MigrationRuntimeOptions::from_millis(Some(10), Some(20));
        let b = a;
        let c = a;
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_ne!(a, MigrationRuntimeOptions::default());
    }

    // -- pre_txn_timeout_sql -----------------------------------------------

    #[test]
    fn pre_txn_timeout_sql_postgres_emits_nothing() {
        let opts = MigrationRuntimeOptions::from_millis(Some(500), Some(1000));
        let sqls = pre_txn_timeout_sql(DatabaseBackend::Postgres, opts);
        assert!(sqls.is_empty());
    }

    #[test]
    fn pre_txn_timeout_sql_mysql_emits_nothing() {
        let opts = MigrationRuntimeOptions::from_millis(Some(500), Some(1000));
        let sqls = pre_txn_timeout_sql(DatabaseBackend::MySql, opts);
        assert!(sqls.is_empty());
    }

    #[test]
    fn pre_txn_timeout_sql_sqlite_with_lock_emits_busy_timeout() {
        let opts = MigrationRuntimeOptions::from_millis(Some(750), None);
        let sqls = pre_txn_timeout_sql(DatabaseBackend::Sqlite, opts);
        assert_eq!(sqls, vec!["PRAGMA busy_timeout = 750".to_owned()]);
    }

    #[test]
    fn pre_txn_timeout_sql_sqlite_without_lock_emits_nothing() {
        let opts = MigrationRuntimeOptions::from_millis(None, Some(9999));
        let sqls = pre_txn_timeout_sql(DatabaseBackend::Sqlite, opts);
        assert!(sqls.is_empty());
    }

    #[test]
    fn pre_txn_timeout_sql_sqlite_default_emits_nothing() {
        let sqls = pre_txn_timeout_sql(DatabaseBackend::Sqlite, MigrationRuntimeOptions::default());
        assert!(sqls.is_empty());
    }

    // -- in_txn_timeout_sql ------------------------------------------------

    #[test]
    fn in_txn_timeout_sql_postgres_both_timeouts() {
        let opts = MigrationRuntimeOptions::from_millis(Some(500), Some(2000));
        let sqls = in_txn_timeout_sql(DatabaseBackend::Postgres, opts);
        assert_eq!(
            sqls,
            vec![
                "SET LOCAL lock_timeout = 500".to_owned(),
                "SET LOCAL statement_timeout = 2000".to_owned(),
            ]
        );
    }

    #[test]
    fn in_txn_timeout_sql_postgres_lock_only() {
        let opts = MigrationRuntimeOptions::from_millis(Some(500), None);
        let sqls = in_txn_timeout_sql(DatabaseBackend::Postgres, opts);
        assert_eq!(sqls, vec!["SET LOCAL lock_timeout = 500".to_owned()]);
    }

    #[test]
    fn in_txn_timeout_sql_postgres_statement_only() {
        let opts = MigrationRuntimeOptions::from_millis(None, Some(2000));
        let sqls = in_txn_timeout_sql(DatabaseBackend::Postgres, opts);
        assert_eq!(sqls, vec!["SET LOCAL statement_timeout = 2000".to_owned()]);
    }

    #[test]
    fn in_txn_timeout_sql_postgres_default_emits_nothing() {
        let sqls = in_txn_timeout_sql(
            DatabaseBackend::Postgres,
            MigrationRuntimeOptions::default(),
        );
        assert!(sqls.is_empty());
    }

    #[test]
    fn in_txn_timeout_sql_mysql_rounds_lock_up_to_seconds() {
        // 1500 ms → div_ceil(1000) = 2 seconds
        let opts = MigrationRuntimeOptions::from_millis(Some(1500), Some(7000));
        let sqls = in_txn_timeout_sql(DatabaseBackend::MySql, opts);
        assert_eq!(
            sqls,
            vec![
                "SET SESSION innodb_lock_wait_timeout = 2".to_owned(),
                "SET SESSION max_execution_time = 7000".to_owned(),
            ]
        );
    }

    #[test]
    fn in_txn_timeout_sql_mysql_sub_second_lock_rounds_up_to_one() {
        // 250 ms → div_ceil(1000) = 1, max(1) = 1 (never collapses to 0)
        let opts = MigrationRuntimeOptions::from_millis(Some(250), None);
        let sqls = in_txn_timeout_sql(DatabaseBackend::MySql, opts);
        assert_eq!(
            sqls,
            vec!["SET SESSION innodb_lock_wait_timeout = 1".to_owned()]
        );
    }

    #[test]
    fn in_txn_timeout_sql_mysql_exact_second_lock() {
        // 1000 ms → div_ceil(1000) = 1
        let opts = MigrationRuntimeOptions::from_millis(Some(1000), None);
        let sqls = in_txn_timeout_sql(DatabaseBackend::MySql, opts);
        assert_eq!(
            sqls,
            vec!["SET SESSION innodb_lock_wait_timeout = 1".to_owned()]
        );
    }

    #[test]
    fn in_txn_timeout_sql_mysql_statement_only() {
        let opts = MigrationRuntimeOptions::from_millis(None, Some(3000));
        let sqls = in_txn_timeout_sql(DatabaseBackend::MySql, opts);
        assert_eq!(
            sqls,
            vec!["SET SESSION max_execution_time = 3000".to_owned()]
        );
    }

    #[test]
    fn in_txn_timeout_sql_mysql_default_emits_nothing() {
        let sqls = in_txn_timeout_sql(DatabaseBackend::MySql, MigrationRuntimeOptions::default());
        assert!(sqls.is_empty());
    }

    #[test]
    fn in_txn_timeout_sql_sqlite_emits_nothing_even_with_both_set() {
        // SQLite lock is handled pre-transaction; no statement timeout.
        let opts = MigrationRuntimeOptions::from_millis(Some(500), Some(2000));
        let sqls = in_txn_timeout_sql(DatabaseBackend::Sqlite, opts);
        assert!(sqls.is_empty());
    }

    // -- collect_version_ids -----------------------------------------------

    #[test]
    fn collect_version_ids_empty_rows_yields_empty_map() {
        let rows: Vec<sea_orm::QueryResult> = Vec::new();
        let ids = collect_version_ids(&rows);
        assert!(ids.is_empty());
    }

    #[test]
    fn collect_version_ids_reads_positive_versions_and_skips_invalid_rows() {
        fn row(version: Option<i32>, id: Option<&str>) -> sea_orm::QueryResult {
            let mut values = std::collections::BTreeMap::new();
            if let Some(version) = version {
                values.insert("version".to_owned(), version.into());
            }
            if let Some(id) = id {
                values.insert("id".to_owned(), id.into());
            }
            sea_orm::ProxyRow::new(values).into()
        }

        let rows = vec![
            row(Some(7), Some("create-users")),
            row(Some(-1), Some("negative")),
            row(None, Some("missing-version")),
            row(Some(8), None),
        ];
        let ids = collect_version_ids(&rows);

        assert_eq!(ids.len(), 2);
        assert_eq!(ids.get(&7).map(String::as_str), Some("create-users"));
        assert_eq!(ids.get(&8).map(String::as_str), Some(""));
        assert!(!ids.contains_key(&u32::MAX));
    }
}
