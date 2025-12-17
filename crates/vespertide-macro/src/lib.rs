// MigrationOptions and MigrationError are now in vespertide-core

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, Token, parse_macro_input};
use vespertide_loader::{load_migrations_at_compile_time, load_models_at_compile_time};
use vespertide_planner::apply_action;
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
    let migrations = match load_migrations_at_compile_time() {
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
    let _models = match load_models_at_compile_time() {
        Ok(models) => models,
        Err(e) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Failed to load models at compile time: {}", e),
            )
            .to_compile_error()
            .into();
        }
    };

    // Build SQL for each migration using incremental baseline schema
    // This is the same approach as cmd_log: start with empty schema and apply each migration
    let mut baseline_schema = Vec::new();
    let mut migration_blocks = Vec::new();

    for migration in &migrations {
        let version = migration.version;

        // Use the current baseline schema (from all previous migrations)
        let queries = match build_plan_queries(migration, &baseline_schema) {
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

        // Update baseline schema incrementally by applying each action
        for action in &migration.actions {
            let _ = apply_action(&mut baseline_schema, action);
        }

        // Pre-generate SQL for all backends at compile time
        // Each query may produce multiple SQL statements, so we flatten them
        let mut pg_sqls = Vec::new();
        let mut mysql_sqls = Vec::new();
        let mut sqlite_sqls = Vec::new();

        for q in &queries {
            for stmt in &q.postgres {
                pg_sqls.push(stmt.build(DatabaseBackend::Postgres));
            }
            for stmt in &q.mysql {
                mysql_sqls.push(stmt.build(DatabaseBackend::MySql));
            }
            for stmt in &q.sqlite {
                sqlite_sqls.push(stmt.build(DatabaseBackend::Sqlite));
            }
        }

        // Generate version guard and SQL execution block
        let block = quote! {
            if version < #version {
                // Begin transaction
                let txn = __pool.begin().await.map_err(|e| {
                    ::vespertide::MigrationError::DatabaseError(format!("Failed to begin transaction: {}", e))
                })?;

                // Select SQL statements based on backend
                let sqls: &[&str] = match backend {
                    sea_orm::DatabaseBackend::Postgres => &[#(#pg_sqls),*],
                    sea_orm::DatabaseBackend::MySql => &[#(#mysql_sqls),*],
                    sea_orm::DatabaseBackend::Sqlite => &[#(#sqlite_sqls),*],
                    _ => &[#(#pg_sqls),*], // Fallback to PostgreSQL syntax for unknown backends
                };

                // Execute SQL statements
                for sql in sqls {
                    if !sql.is_empty() {
                        let stmt = sea_orm::Statement::from_string(backend, *sql);
                        txn.execute_raw(stmt).await.map_err(|e| {
                            ::vespertide::MigrationError::DatabaseError(format!("Failed to execute SQL '{}': {}", sql, e))
                        })?;
                    }
                }

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
