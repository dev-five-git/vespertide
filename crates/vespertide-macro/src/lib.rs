// MigrationOptions and MigrationError are now in vespertide-core

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Expr, Ident, Token};
use syn::parse::{Parse, ParseStream};
use std::env;
use std::fs;
use std::path::PathBuf;
use vespertide_config::VespertideConfig;
use vespertide_core::MigrationPlan;
use vespertide_query::build_plan_queries;

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
    let version_table = input.version_table.unwrap_or_else(|| "vespertide_version".to_string());

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

    // Build SQL for each migration
    let mut migration_blocks = Vec::new();
    for migration in &migrations {
        let version = migration.version;
        let queries = match build_plan_queries(migration) {
            Ok(queries) => queries,
            Err(e) => {
                return syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("Failed to build queries for migration version {}: {}", version, e),
                )
                .to_compile_error()
                .into();
            }
        };

        // Statically embed SQL text and bind parameters (as values)
        let sql_statements: Vec<_> = queries
            .iter()
            .map(|q| {
                let sql = &q.sql;
                let binds = &q.binds;
                let value_tokens = binds.iter().map(|b| {
                    quote! { sea_orm::Value::String(Some(#b.to_string())) }
                });
                quote! { (#sql, vec![#(#value_tokens),*]) }
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
                        let (sql, values) = #sql_statements;
                        let stmt = sea_orm::Statement::from_sql_and_values(backend, sql, values);
                        txn.execute_raw(stmt).await.map_err(|e| {
                            ::vespertide::MigrationError::DatabaseError(format!("Failed to execute SQL: {}", e))
                        })?;
                    }
                )*

                // Insert version record for this migration
                let stmt = sea_orm::Statement::from_sql_and_values(
                    backend,
                    &format!("INSERT INTO {} (version) VALUES (?)", version_table),
                    vec![sea_orm::Value::Int(Some(#version as i32))],
                );
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
            let create_table_sql = format!(
                "CREATE TABLE IF NOT EXISTS {} (version INTEGER PRIMARY KEY, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)",
                version_table
            );
            let stmt = sea_orm::Statement::from_string(backend, create_table_sql);
            __pool.execute_raw(stmt).await.map_err(|e| {
                ::vespertide::MigrationError::DatabaseError(format!("Failed to create version table: {}", e))
            })?;

            // Read current maximum version (latest applied migration)
            let stmt = sea_orm::Statement::from_string(
                backend,
                format!("SELECT MAX(version) as version FROM {}", version_table),
            );
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

fn load_migrations_at_compile_time() -> Result<Vec<MigrationPlan>, Box<dyn std::error::Error>> {
    // Locate project root from CARGO_MANIFEST_DIR
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| "CARGO_MANIFEST_DIR environment variable not set")?;
    let project_root = PathBuf::from(manifest_dir);

    // Read vespertide.json
    let config_path = project_root.join("vespertide.json");
    let config: VespertideConfig = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        serde_json::from_str(&content)?
    } else {
        // Fall back to defaults if config is missing
        VespertideConfig::default()
    };

    // Read migrations directory
    let migrations_dir = project_root.join(config.migrations_dir());
    if !migrations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plans = Vec::new();
    let entries = fs::read_dir(&migrations_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let ext = path.extension().and_then(|s| s.to_str());
            if ext == Some("json") || ext == Some("yaml") || ext == Some("yml") {
                let content = fs::read_to_string(&path)?;

                let plan: MigrationPlan = if ext == Some("json") {
                    serde_json::from_str(&content)?
                } else {
                    serde_yaml::from_str(&content)?
                };

                plans.push(plan);
            }
        }
    }

    // Sort by version
    plans.sort_by_key(|p| p.version);
    Ok(plans)
}
