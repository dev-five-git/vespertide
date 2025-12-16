// MigrationOptions and MigrationError are now in vespertide-core

mod loader;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, Token, parse_macro_input};
use vespertide_query::{DatabaseBackend, build_plan_queries};

struct MacroInput {
    pool: Expr,
    version_table: Option<String>,
}

impl Parse for MacroInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let pool = input.parse()?;
        let mut version_table = None;

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }

            let key: Ident = input.parse()?;
            if key == "version_table" {
                input.parse::<Token![=]>()?;
                let value: syn::LitStr = input.parse()?;
                version_table = Some(value.value());
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "unsupported option for vespertide_migration!",
                ));
            }
        }

        Ok(MacroInput {
            pool,
            version_table,
        })
    }
}

/// Zero-runtime migration entry point.
#[proc_macro]
pub fn vespertide_migration(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as MacroInput);
    let pool = &input.pool;
    let version_table = input
        .version_table
        .unwrap_or_else(|| "vespertide_version".to_string());

    // Load migration files and build SQL at compile time
    let migrations = match loader::load_migrations_at_compile_time() {
        Ok(migrations) => migrations,
        Err(e) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Failed to load migrations at compile time: {}", e),
            )
            .to_compile_error()
            .into();
        }
    };

    // Build SQL for each migration
    let mut migration_blocks = Vec::new();
    for migration in &migrations {
        let version = migration.version;
        let queries = match build_plan_queries(migration) {
            Ok(queries) => queries,
            Err(e) => {
                return syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "Failed to build queries for migration version {}: {}",
                        version, e
                    ),
                )
                .to_compile_error()
                .into();
            }
        };

        // Pre-generate SQL for all backends at compile time
        let sql_statements: Vec<_> = queries
            .iter()
            .map(|q| {
                let pg_sql = q.postgres.iter().map(|q| q.build(DatabaseBackend::Postgres)).collect::<Vec<_>>().join(";\n");
                let mysql_sql = q.mysql.iter().map(|q| q.build(DatabaseBackend::MySql)).collect::<Vec<_>>().join(";\n");
                let sqlite_sql = q.sqlite.iter().map(|q| q.build(DatabaseBackend::Sqlite)).collect::<Vec<_>>().join(";\n");

                quote! {
                    match backend {
                        sea_orm::DatabaseBackend::Postgres => #pg_sql,
                        sea_orm::DatabaseBackend::MySql => #mysql_sql,
                        sea_orm::DatabaseBackend::Sqlite => #sqlite_sql,
                        _ => #pg_sql, // Fallback to PostgreSQL syntax for unknown backends
                    }
                }
            })
            .collect();

        // Generate version guard and SQL execution block
        let block = quote! {
            if version < #version {
                // Begin transaction
                let txn = __pool.begin().await.map_err(|e| {
                    ::vespertide::MigrationError::DatabaseError(format!("Failed to begin transaction: {}", e))
                })?;

                // Execute SQL statements
                #(
                    {
                        let sql: &str = #sql_statements;
                        let stmt = sea_orm::Statement::from_string(backend, sql);
                        txn.execute_raw(stmt).await.map_err(|e| {
                            ::vespertide::MigrationError::DatabaseError(format!("Failed to execute SQL '{}': {}", sql, e))
                        })?;
                    }
                )*

                // Insert version record for this migration
                let q = if matches!(backend, sea_orm::DatabaseBackend::MySql) { '`' } else { '"' };
                let insert_sql = format!("INSERT INTO {q}{}{q} (version) VALUES ({})", version_table, #version);
                let stmt = sea_orm::Statement::from_string(backend, insert_sql);
                txn.execute_raw(stmt).await.map_err(|e| {
                    ::vespertide::MigrationError::DatabaseError(format!("Failed to insert version: {}", e))
                })?;

                // Commit transaction
                txn.commit().await.map_err(|e| {
                    ::vespertide::MigrationError::DatabaseError(format!("Failed to commit transaction: {}", e))
                })?;
            }
        };

        migration_blocks.push(block);
    }

    // Emit final generated async block
    let generated = quote! {
        async {
            use sea_orm::{ConnectionTrait, TransactionTrait};
            let __pool = #pool;
            let version_table = #version_table;
            let backend = __pool.get_database_backend();

            // Create version table if it does not exist
            // Table structure: version (INTEGER PRIMARY KEY), created_at (timestamp)
            let q = if matches!(backend, sea_orm::DatabaseBackend::MySql) { '`' } else { '"' };
            let create_table_sql = format!(
                "CREATE TABLE IF NOT EXISTS {q}{}{q} (version INTEGER PRIMARY KEY, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)",
                version_table
            );
            let stmt = sea_orm::Statement::from_string(backend, create_table_sql);
            __pool.execute_raw(stmt).await.map_err(|e| {
                ::vespertide::MigrationError::DatabaseError(format!("Failed to create version table: {}", e))
            })?;

            // Read current maximum version (latest applied migration)
            let select_sql = format!("SELECT MAX(version) as version FROM {q}{}{q}", version_table);
            let stmt = sea_orm::Statement::from_string(backend, select_sql);
            let version_result = __pool.query_one_raw(stmt).await.map_err(|e| {
                ::vespertide::MigrationError::DatabaseError(format!("Failed to read version: {}", e))
            })?;

            let mut version = version_result
                .and_then(|row| row.try_get::<i32>("", "version").ok())
                .unwrap_or(0) as u32;

            // Execute each migration block
            #(#migration_blocks)*

            Ok::<(), ::vespertide::MigrationError>(())
        }
    };

    generated.into()
}
