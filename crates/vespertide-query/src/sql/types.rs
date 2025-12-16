/// Database backend for SQL generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    Postgres,
    MySql,
    Sqlite,
}

/// Represents a built query that can be converted to SQL for any database backend
#[derive(Debug, Clone)]
pub enum BuiltQuery {
    CreateTable(Box<sea_query::TableCreateStatement>),
    DropTable(Box<sea_query::TableDropStatement>),
    AlterTable(Box<sea_query::TableAlterStatement>),
    CreateIndex(Box<sea_query::IndexCreateStatement>),
    DropIndex(Box<sea_query::IndexDropStatement>),
    RenameTable(Box<sea_query::TableRenameStatement>),
    CreateForeignKey(Box<sea_query::ForeignKeyCreateStatement>),
    DropForeignKey(Box<sea_query::ForeignKeyDropStatement>),
    Raw(RawSql),
}

/// Raw SQL that may have backend-specific variants
#[derive(Debug, Clone)]
pub struct RawSql {
    pub postgres: String,
    pub mysql: String,
    pub sqlite: String,
}

impl RawSql {
    /// Create a RawSql with the same SQL for all backends
    pub fn uniform(sql: String) -> Self {
        Self {
            postgres: sql.clone(),
            mysql: sql.clone(),
            sqlite: sql,
        }
    }

    /// Create a RawSql with different SQL for each backend
    pub fn per_backend(postgres: String, mysql: String, sqlite: String) -> Self {
        Self {
            postgres,
            mysql,
            sqlite,
        }
    }
}

impl BuiltQuery {
    /// Build SQL string for the specified database backend
    pub fn build(&self, backend: DatabaseBackend) -> String {
        match self {
            BuiltQuery::CreateTable(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::DropTable(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::AlterTable(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::CreateIndex(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::DropIndex(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::RenameTable(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::CreateForeignKey(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::DropForeignKey(stmt) => crate::sql::helpers::build_schema_statement(stmt.as_ref(), backend),
            BuiltQuery::Raw(raw) => match backend {
                DatabaseBackend::Postgres => raw.postgres.clone(),
                DatabaseBackend::MySql => raw.mysql.clone(),
                DatabaseBackend::Sqlite => raw.sqlite.clone(),
            },
        }
    }
}
