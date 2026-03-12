use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use crate::MigrationError;

#[derive(Debug, Clone, Copy)]
pub struct EmbeddedMigration {
    pub version: u32,
    pub migration_id: &'static str,
    pub comment: &'static str,
    pub postgres_sql_blob: &'static str,
    pub mysql_sql_blob: &'static str,
    pub sqlite_sql_blob: &'static str,
}

impl EmbeddedMigration {
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
        if matches!(backend, DatabaseBackend::MySql) {
            self.mysql_sql_blob
        } else if matches!(backend, DatabaseBackend::Sqlite) {
            self.sqlite_sql_blob
        } else {
            self.postgres_sql_blob
        }
    }
}

pub fn split_sql_blob(blob: &str) -> impl Iterator<Item = &str> {
    blob.split_terminator('\0').filter(|sql| !sql.is_empty())
}

pub async fn run_embedded_migrations(
    pool: &DatabaseConnection,
    version_table: &str,
    verbose: bool,
    migrations: &[EmbeddedMigration],
) -> Result<(), MigrationError> {
    let backend = pool.get_database_backend();
    let q = if matches!(backend, DatabaseBackend::MySql) {
        '`'
    } else {
        '"'
    };

    let create_table_sql = format!(
        "CREATE TABLE IF NOT EXISTS {q}{}{q} (version INTEGER PRIMARY KEY, id TEXT DEFAULT '', created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)",
        version_table
    );
    let stmt = Statement::from_string(backend, create_table_sql);
    pool.execute_raw(stmt).await.map_err(|e| {
        MigrationError::DatabaseError(format!("Failed to create version table: {}", e))
    })?;

    let alter_sql = format!(
        "ALTER TABLE {q}{}{q} ADD COLUMN id TEXT DEFAULT ''",
        version_table
    );
    let stmt = Statement::from_string(backend, alter_sql);
    let _ = pool.execute_raw(stmt).await;

    let txn = pool.begin().await.map_err(|e| {
        MigrationError::DatabaseError(format!("Failed to begin transaction: {}", e))
    })?;

    let select_sql = format!(
        "SELECT MAX(version) as version FROM {q}{}{q}",
        version_table
    );
    let stmt = Statement::from_string(backend, select_sql);
    let version_result = txn
        .query_one_raw(stmt)
        .await
        .map_err(|e| MigrationError::DatabaseError(format!("Failed to read version: {}", e)))?;
    let version = version_result
        .and_then(|row| row.try_get::<i32>("", "version").ok())
        .unwrap_or(0) as u32;

    let select_ids_sql = format!("SELECT version, id FROM {q}{}{q}", version_table);
    let stmt = Statement::from_string(backend, select_ids_sql);
    let id_rows = txn
        .query_all_raw(stmt)
        .await
        .map_err(|e| MigrationError::DatabaseError(format!("Failed to read version ids: {}", e)))?;
    let mut version_ids = std::collections::HashMap::<u32, String>::new();
    for row in &id_rows {
        if let Ok(found_version) = row.try_get::<i32>("", "version") {
            let id = row.try_get::<String>("", "id").unwrap_or_default();
            version_ids.insert(found_version as u32, id);
        }
    }

    if verbose {
        eprintln!("[vespertide] Current database version: {}", version);
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
            txn.execute_raw(stmt).await.map_err(|e| {
                MigrationError::DatabaseError(format!("Failed to execute SQL '{}': {}", sql, e))
            })?;
        }

        let insert_sql = format!(
            "INSERT INTO {q}{}{q} (version, id) VALUES ({}, '{}')",
            version_table, migration.version, migration.migration_id
        );
        let stmt = Statement::from_string(backend, insert_sql);
        txn.execute_raw(stmt).await.map_err(|e| {
            MigrationError::DatabaseError(format!("Failed to insert version: {}", e))
        })?;

        if verbose {
            eprintln!(
                "[vespertide] Migration v{} applied successfully",
                migration.version
            );
        }
    }

    txn.commit().await.map_err(|e| {
        MigrationError::DatabaseError(format!("Failed to commit transaction: {}", e))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};

    use crate::MigrationError;

    use super::{EmbeddedMigration, run_embedded_migrations, split_sql_blob};

    async fn sqlite_memory_db() -> sea_orm::DatabaseConnection {
        Database::connect("sqlite::memory:").await.unwrap()
    }

    async fn read_versions(db: &sea_orm::DatabaseConnection) -> Vec<(i32, String)> {
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

    #[test]
    fn split_sql_blob_ignores_empty_segments() {
        let sqls: Vec<_> =
            split_sql_blob("CREATE TABLE users ();\0\0ALTER TABLE users;\0").collect();

        assert_eq!(sqls, vec!["CREATE TABLE users ();", "ALTER TABLE users;"]);
    }

    #[test]
    fn embedded_migration_selects_backend_blob() {
        let migration = EmbeddedMigration::new(1, "id", "comment", "pg\0", "mysql\0", "sqlite\0");

        assert_eq!(migration.sql_blob(DatabaseBackend::Postgres), "pg\0");
        assert_eq!(migration.sql_blob(DatabaseBackend::MySql), "mysql\0");
        assert_eq!(migration.sql_blob(DatabaseBackend::Sqlite), "sqlite\0");
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
        let stmt = Statement::from_string(DatabaseBackend::Sqlite, "CREATE TABLE \"vespertide_migrations\" (version INTEGER PRIMARY KEY, id TEXT DEFAULT '', created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)".to_owned());
        db.execute_raw(stmt).await.unwrap();
        let stmt = Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO \"vespertide_migrations\" (version, id) VALUES (2, 'different')"
                .to_owned(),
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

        assert!(
            matches!(result, Err(MigrationError::DatabaseError(message)) if message.contains("Failed to execute SQL 'THIS IS NOT SQL;'"))
        );
    }

    #[tokio::test]
    async fn run_embedded_migrations_detects_existing_version_id_mismatch() {
        let db = sqlite_memory_db().await;
        let stmt = Statement::from_string(DatabaseBackend::Sqlite, "CREATE TABLE \"vespertide_migrations\" (version INTEGER PRIMARY KEY, id TEXT DEFAULT '', created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)".to_owned());
        db.execute_raw(stmt).await.unwrap();
        let stmt = Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO \"vespertide_migrations\" (version, id) VALUES (2, 'different')"
                .to_owned(),
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
}
