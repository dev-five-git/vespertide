//! Integration tests for `vespertide::runtime` that exercise
//! `Database::connect("sqlite::memory:")`. These live in the workspace-
//! excluded `tests/runtime-sqlite` crate because sea-orm's `sqlx-sqlite`
//! feature pulls in libsqlite3-sys 0.30, which collides on
//! `links = "sqlite3"` with vespertide-query's rusqlite 0.39 dev-dep
//! (libsqlite3-sys 0.37) under unified workspace resolution.
//!
//! Run via: `cargo test --manifest-path tests/runtime-sqlite/Cargo.toml`.

use std::error::Error;

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
use vespertide::{
    MigrationError,
    runtime::{
        EmbeddedMigration, MigrationRuntimeOptions, run_embedded_migrations,
        run_embedded_migrations_with_options,
    },
};

async fn sqlite_memory_db() -> DatabaseConnection {
    Database::connect("sqlite::memory:").await.unwrap()
}

async fn read_versions(db: &DatabaseConnection) -> Vec<(i32, String)> {
    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT version, id FROM \"vespertide_migrations\" ORDER BY version".to_owned(),
    );
    let rows = db.query_all_raw(stmt).await.unwrap();
    rows.into_iter()
        .map(|row| {
            (
                row.try_get::<i32>("", "version").unwrap(),
                row.try_get::<String>("", "id").unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn run_embedded_migrations_applies_pending_versions_and_records_ids() {
    let db = sqlite_memory_db().await;
    let migrations = [
        EmbeddedMigration::new(
            1,
            "init",
            "create users",
            "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
            "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
            "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
        ),
        EmbeddedMigration::new(
            2,
            "add_name",
            "add name column",
            "ALTER TABLE users ADD COLUMN name TEXT;\0",
            "ALTER TABLE users ADD COLUMN name TEXT;\0",
            "ALTER TABLE users ADD COLUMN name TEXT;\0",
        ),
    ];

    run_embedded_migrations(&db, "vespertide_migrations", true, &migrations)
        .await
        .unwrap();

    let versions = read_versions(&db).await;
    assert_eq!(
        versions,
        vec![(1, "init".to_string()), (2, "add_name".to_string())]
    );

    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA table_info('users')".to_owned(),
    );
    let rows = db.query_all_raw(stmt).await.unwrap();
    let names: Vec<_> = rows
        .into_iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert_eq!(names, vec!["id".to_string(), "name".to_string()]);
}

async fn read_busy_timeout(db: &DatabaseConnection) -> i64 {
    let stmt = Statement::from_string(DatabaseBackend::Sqlite, "PRAGMA busy_timeout".to_owned());
    let row = db.query_one_raw(stmt).await.unwrap().expect("busy_timeout row");
    row.try_get::<i32>("", "timeout")
        .map(i64::from)
        .or_else(|_| row.try_get::<i64>("", "timeout"))
        .expect("busy_timeout value")
}

/// F94: a configured `lock_timeout_ms` must apply `PRAGMA busy_timeout` on
/// SQLite at the start of the migration session, and the migration must
/// still apply successfully.
#[tokio::test]
async fn with_options_applies_sqlite_busy_timeout() {
    let db = sqlite_memory_db().await;

    let migrations = [EmbeddedMigration::new(
        1,
        "init",
        "create users",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
    )];

    run_embedded_migrations_with_options(
        &db,
        "vespertide_migrations",
        true,
        &migrations,
        MigrationRuntimeOptions::from_millis(Some(5000), Some(30000)),
    )
    .await
    .unwrap();

    // The migration applied.
    assert_eq!(read_versions(&db).await, vec![(1, "init".to_string())]);

    // busy_timeout was set to the configured lock timeout. (statement_timeout
    // is silently ignored on SQLite, which must not error.)
    assert_eq!(
        read_busy_timeout(&db).await,
        5000,
        "lock_timeout_ms must set SQLite PRAGMA busy_timeout"
    );
}

/// F94: the default (no-timeout) `run_embedded_migrations` must NOT touch
/// busy_timeout — proving the feature is opt-in and the delegation path is
/// behaviour-preserving.
#[tokio::test]
async fn default_run_does_not_set_busy_timeout() {
    let db = sqlite_memory_db().await;
    let migrations = [EmbeddedMigration::new(
        1,
        "init",
        "create users",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
    )];

    let baseline = read_busy_timeout(&db).await;
    run_embedded_migrations(&db, "vespertide_migrations", false, &migrations)
        .await
        .unwrap();

    assert_eq!(
        read_busy_timeout(&db).await,
        baseline,
        "default run must leave busy_timeout untouched"
    );
}

#[tokio::test]
async fn run_embedded_migrations_skips_versions_that_are_already_applied() {
    let db = sqlite_memory_db().await;
    run_embedded_migrations(
        &db,
        "vespertide_migrations",
        false,
        &[EmbeddedMigration::new(
            1,
            "init",
            "create users",
            "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
            "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
            "CREATE TABLE users (id INTEGER PRIMARY KEY);\0",
        )],
    )
    .await
    .unwrap();

    run_embedded_migrations(
        &db,
        "vespertide_migrations",
        true,
        &[
            EmbeddedMigration::new(
                1,
                "init",
                "should skip existing",
                "ALTER TABLE users ADD COLUMN skipped TEXT;\0",
                "ALTER TABLE users ADD COLUMN skipped TEXT;\0",
                "ALTER TABLE users ADD COLUMN skipped TEXT;\0",
            ),
            EmbeddedMigration::new(
                2,
                "add_name",
                "apply only new version",
                "ALTER TABLE users ADD COLUMN name TEXT;\0",
                "ALTER TABLE users ADD COLUMN name TEXT;\0",
                "ALTER TABLE users ADD COLUMN name TEXT;\0",
            ),
        ],
    )
    .await
    .unwrap();

    let versions = read_versions(&db).await;
    assert_eq!(
        versions,
        vec![(1, "init".to_string()), (2, "add_name".to_string())]
    );

    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA table_info('users')".to_owned(),
    );
    let rows = db.query_all_raw(stmt).await.unwrap();
    let names: Vec<_> = rows
        .into_iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert!(!names.iter().any(|name| name == "skipped"));
    assert!(names.iter().any(|name| name == "name"));
}

#[tokio::test]
async fn run_embedded_migrations_surfaces_sql_errors() {
    let db = sqlite_memory_db().await;
    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "CREATE TABLE \"vespertide_migrations\" (version INTEGER PRIMARY KEY, id TEXT DEFAULT '', created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)".to_owned(),
    );
    db.execute_raw(stmt).await.unwrap();
    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "INSERT INTO \"vespertide_migrations\" (version, id) VALUES (2, 'different')".to_owned(),
    );
    db.execute_raw(stmt).await.unwrap();

    let result = run_embedded_migrations(
        &db,
        "vespertide_migrations",
        true,
        &[EmbeddedMigration::new(
            3,
            "broken",
            "invalid sql",
            "THIS IS NOT SQL;\0",
            "THIS IS NOT SQL;\0",
            "THIS IS NOT SQL;\0",
        )],
    )
    .await;

    let err = result.unwrap_err();
    assert!(err.source().is_some());
    assert!(matches!(
        err,
        MigrationError::Database { message, source: Some(_) }
            if message.contains("Failed to execute SQL 'THIS IS NOT SQL;'")
    ));
}

#[tokio::test]
async fn run_embedded_migrations_detects_existing_version_id_mismatch() {
    let db = sqlite_memory_db().await;
    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "CREATE TABLE \"vespertide_migrations\" (version INTEGER PRIMARY KEY, id TEXT DEFAULT '', created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)".to_owned(),
    );
    db.execute_raw(stmt).await.unwrap();
    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "INSERT INTO \"vespertide_migrations\" (version, id) VALUES (2, 'different')".to_owned(),
    );
    db.execute_raw(stmt).await.unwrap();
    let stmt = Statement::from_string(
        DatabaseBackend::Sqlite,
        "INSERT INTO \"vespertide_migrations\" (version, id) VALUES (2147483648, 'overflow')"
            .to_owned(),
    );
    db.execute_raw(stmt).await.unwrap();

    let result = run_embedded_migrations(
        &db,
        "vespertide_migrations",
        true,
        &[EmbeddedMigration::new(
            2,
            "expected",
            "mismatch",
            "ALTER TABLE users ADD COLUMN name TEXT;\0",
            "ALTER TABLE users ADD COLUMN name TEXT;\0",
            "ALTER TABLE users ADD COLUMN name TEXT;\0",
        )],
    )
    .await;

    assert!(matches!(
        result,
        Err(MigrationError::IdMismatch {
            version: 2,
            expected,
            found,
        }) if expected == "expected" && found == "different"
    ));
}

#[tokio::test]
async fn run_embedded_migrations_preserves_db_error_source() {
    let db = sqlite_memory_db().await;

    let err = run_embedded_migrations(
        &db,
        "vespertide_migrations",
        false,
        &[EmbeddedMigration::new(
            1,
            "broken",
            "invalid sql",
            "THIS IS NOT SQL;\0",
            "THIS IS NOT SQL;\0",
            "THIS IS NOT SQL;\0",
        )],
    )
    .await
    .unwrap_err();

    assert!(err.source().is_some());
    let source = err.source().unwrap();
    assert!(source.downcast_ref::<sea_orm::DbErr>().is_some());
    assert!(matches!(
        err,
        MigrationError::Database { message, source: Some(_) }
            if message.contains("Failed to execute SQL 'THIS IS NOT SQL;'")
    ));
}
