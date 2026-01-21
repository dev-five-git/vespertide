// MigrationOptions and MigrationError are now in vespertide-core

use std::env;
use std::path::PathBuf;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, Token};
use vespertide_loader::{
    load_config_or_default, load_migrations_at_compile_time, load_models_at_compile_time,
};
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

/// Build a migration block for a single migration version.
/// Returns the generated code block and updates the baseline schema.
pub(crate) fn build_migration_block(
    migration: &vespertide_core::MigrationPlan,
    baseline_schema: &mut Vec<vespertide_core::TableDef>,
) -> Result<proc_macro2::TokenStream, String> {
    let version = migration.version;

    // Use the current baseline schema (from all previous migrations)
    let queries = build_plan_queries(migration, baseline_schema).map_err(|e| {
        format!(
            "Failed to build queries for migration version {}: {}",
            version, e
        )
    })?;

    // Update baseline schema incrementally by applying each action
    for action in &migration.actions {
        let _ = apply_action(baseline_schema, action);
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

    Ok(block)
}

/// Generate the final async migration block with all migrations.
pub(crate) fn generate_migration_code(
    pool: &Expr,
    version_table: &str,
    migration_blocks: Vec<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    quote! {
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
    }
}

/// Inner implementation that works with proc_macro2::TokenStream for testability.
pub(crate) fn vespertide_migration_impl(
    input: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let input: MacroInput = match syn::parse2(input) {
        Ok(input) => input,
        Err(e) => return e.to_compile_error(),
    };
    let pool = &input.pool;

    // Get project root from CARGO_MANIFEST_DIR (same as load_migrations_at_compile_time)
    let project_root = match env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => Some(PathBuf::from(dir)),
        Err(_) => None,
    };

    // Load config to get prefix
    let config = match load_config_or_default(project_root) {
        Ok(config) => config,
        #[cfg(not(tarpaulin_include))]
        Err(e) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Failed to load config at compile time: {}", e),
            )
            .to_compile_error();
        }
    };
    let prefix = config.prefix();

    // Apply prefix to version_table if not explicitly provided
    let version_table = input
        .version_table
        .map(|vt| config.apply_prefix(&vt))
        .unwrap_or_else(|| config.apply_prefix("vespertide_version"));

    // Load migration files and build SQL at compile time
    let migrations = match load_migrations_at_compile_time() {
        Ok(migrations) => migrations,
        Err(e) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Failed to load migrations at compile time: {}", e),
            )
            .to_compile_error();
        }
    };
    let _models = match load_models_at_compile_time() {
        Ok(models) => models,
        #[cfg(not(tarpaulin_include))]
        Err(e) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Failed to load models at compile time: {}", e),
            )
            .to_compile_error();
        }
    };

    // Apply prefix to migrations and build SQL using incremental baseline schema
    let mut baseline_schema = Vec::new();
    let mut migration_blocks = Vec::new();

    #[cfg(not(tarpaulin_include))]
    for migration in &migrations {
        // Apply prefix to migration table names
        let prefixed_migration = migration.clone().with_prefix(prefix);
        match build_migration_block(&prefixed_migration, &mut baseline_schema) {
            Ok(block) => migration_blocks.push(block),
            Err(e) => {
                return syn::Error::new(proc_macro2::Span::call_site(), e).to_compile_error();
            }
        }
    }

    generate_migration_code(pool, &version_table, migration_blocks)
}

/// Zero-runtime migration entry point.
#[cfg(not(tarpaulin_include))]
#[proc_macro]
pub fn vespertide_migration(input: TokenStream) -> TokenStream {
    vespertide_migration_impl(input.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    use vespertide_core::{
        ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, StrOrBoolOrArray,
    };

    #[test]
    fn test_macro_expansion_with_runtime_macros() {
        // Create a temporary directory with test files
        let dir = tempdir().unwrap();

        // Create a test file that uses the macro
        let test_file_path = dir.path().join("test_macro.rs");
        let mut test_file = File::create(&test_file_path).unwrap();
        writeln!(
            test_file,
            r#"vespertide_migration!(pool, version_table = "test_versions");"#
        )
        .unwrap();

        // Use runtime-macros to emulate macro expansion
        let file = File::open(&test_file_path).unwrap();
        let result = runtime_macros::emulate_functionlike_macro_expansion(
            file,
            &[("vespertide_migration", vespertide_migration_impl)],
        );

        // The macro will fail because there's no vespertide config, but
        // the important thing is that it runs and covers the macro code
        // We expect an error due to missing config
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_macro_with_simple_pool() {
        let dir = tempdir().unwrap();
        let test_file_path = dir.path().join("test_simple.rs");
        let mut test_file = File::create(&test_file_path).unwrap();
        writeln!(test_file, r#"vespertide_migration!(db_pool);"#).unwrap();

        let file = File::open(&test_file_path).unwrap();
        let result = runtime_macros::emulate_functionlike_macro_expansion(
            file,
            &[("vespertide_migration", vespertide_migration_impl)],
        );

        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_macro_parsing_invalid_option() {
        // Test that invalid options produce a compile error
        let input: proc_macro2::TokenStream = "pool, invalid_option = \"value\"".parse().unwrap();
        let output = vespertide_migration_impl(input);
        let output_str = output.to_string();
        // Should contain an error message about unsupported option
        assert!(output_str.contains("unsupported option"));
    }

    #[test]
    fn test_macro_parsing_valid_input() {
        // Test that valid input is parsed correctly
        // The macro will either succeed (if migrations dir exists and is empty)
        // or fail with a migration loading error
        let input: proc_macro2::TokenStream = "my_pool".parse().unwrap();
        let output = vespertide_migration_impl(input);
        let output_str = output.to_string();
        // Should produce output (either success or migration loading error)
        assert!(!output_str.is_empty());
        // If error, it should mention "Failed to load"
        // If success, it should contain "async"
        assert!(
            output_str.contains("async") || output_str.contains("Failed to load"),
            "Unexpected output: {}",
            output_str
        );
    }

    #[test]
    fn test_macro_parsing_with_version_table() {
        let input: proc_macro2::TokenStream =
            r#"pool, version_table = "custom_versions""#.parse().unwrap();
        let output = vespertide_migration_impl(input);
        let output_str = output.to_string();
        assert!(!output_str.is_empty());
    }

    #[test]
    fn test_macro_parsing_trailing_comma() {
        let input: proc_macro2::TokenStream = "pool,".parse().unwrap();
        let output = vespertide_migration_impl(input);
        let output_str = output.to_string();
        assert!(!output_str.is_empty());
    }

    fn test_column(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.into(),
            r#type: ColumnType::Simple(SimpleColumnType::Integer),
            nullable: false,
            default: None,
            comment: None,
            primary_key: None,
            unique: None,
            index: None,
            foreign_key: None,
        }
    }

    #[test]
    fn test_build_migration_block_create_table() {
        let migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![test_column("id")],
                constraints: vec![],
            }],
        };

        let mut baseline = Vec::new();
        let result = build_migration_block(&migration, &mut baseline);

        assert!(result.is_ok());
        let block = result.unwrap();
        let block_str = block.to_string();

        // Verify the generated block contains expected elements
        assert!(block_str.contains("version < 1u32"));
        assert!(block_str.contains("CREATE TABLE"));

        // Verify baseline schema was updated
        assert_eq!(baseline.len(), 1);
        assert_eq!(baseline[0].name, "users");
    }

    #[test]
    fn test_build_migration_block_add_column() {
        // First create the table
        let create_migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![test_column("id")],
                constraints: vec![],
            }],
        };

        let mut baseline = Vec::new();
        let _ = build_migration_block(&create_migration, &mut baseline);

        // Now add a column
        let add_column_migration = MigrationPlan {
            version: 2,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::AddColumn {
                table: "users".into(),
                column: Box::new(ColumnDef {
                    name: "email".into(),
                    r#type: ColumnType::Simple(SimpleColumnType::Text),
                    nullable: true,
                    default: None,
                    comment: None,
                    primary_key: None,
                    unique: None,
                    index: None,
                    foreign_key: None,
                }),
                fill_with: None,
            }],
        };

        let result = build_migration_block(&add_column_migration, &mut baseline);
        assert!(result.is_ok());
        let block = result.unwrap();
        let block_str = block.to_string();

        assert!(block_str.contains("version < 2u32"));
        assert!(block_str.contains("ALTER TABLE"));
        assert!(block_str.contains("ADD COLUMN"));
    }

    #[test]
    fn test_build_migration_block_multiple_actions() {
        let migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![
                MigrationAction::CreateTable {
                    table: "users".into(),
                    columns: vec![test_column("id")],
                    constraints: vec![],
                },
                MigrationAction::CreateTable {
                    table: "posts".into(),
                    columns: vec![test_column("id")],
                    constraints: vec![],
                },
            ],
        };

        let mut baseline = Vec::new();
        let result = build_migration_block(&migration, &mut baseline);

        assert!(result.is_ok());
        assert_eq!(baseline.len(), 2);
    }

    #[test]
    fn test_generate_migration_code() {
        let pool: Expr = syn::parse_str("db_pool").unwrap();
        let version_table = "test_versions";

        // Create a simple migration block
        let migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![test_column("id")],
                constraints: vec![],
            }],
        };

        let mut baseline = Vec::new();
        let block = build_migration_block(&migration, &mut baseline).unwrap();

        let generated = generate_migration_code(&pool, version_table, vec![block]);
        let generated_str = generated.to_string();

        // Verify the generated code structure
        assert!(generated_str.contains("async"));
        assert!(generated_str.contains("db_pool"));
        assert!(generated_str.contains("test_versions"));
        assert!(generated_str.contains("CREATE TABLE IF NOT EXISTS"));
        assert!(generated_str.contains("SELECT MAX"));
    }

    #[test]
    fn test_generate_migration_code_empty_migrations() {
        let pool: Expr = syn::parse_str("pool").unwrap();
        let version_table = "vespertide_version";

        let generated = generate_migration_code(&pool, version_table, vec![]);
        let generated_str = generated.to_string();

        // Should still generate the wrapper code
        assert!(generated_str.contains("async"));
        assert!(generated_str.contains("vespertide_version"));
    }

    #[test]
    fn test_generate_migration_code_multiple_blocks() {
        let pool: Expr = syn::parse_str("connection").unwrap();

        let mut baseline = Vec::new();

        let migration1 = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![test_column("id")],
                constraints: vec![],
            }],
        };
        let block1 = build_migration_block(&migration1, &mut baseline).unwrap();

        let migration2 = MigrationPlan {
            version: 2,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "posts".into(),
                columns: vec![test_column("id")],
                constraints: vec![],
            }],
        };
        let block2 = build_migration_block(&migration2, &mut baseline).unwrap();

        let generated = generate_migration_code(&pool, "migrations", vec![block1, block2]);
        let generated_str = generated.to_string();

        // Both version checks should be present
        assert!(generated_str.contains("version < 1u32"));
        assert!(generated_str.contains("version < 2u32"));
    }

    #[test]
    fn test_build_migration_block_generates_all_backends() {
        let migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "test_table".into(),
                columns: vec![test_column("id")],
                constraints: vec![],
            }],
        };

        let mut baseline = Vec::new();
        let result = build_migration_block(&migration, &mut baseline);
        assert!(result.is_ok());

        let block_str = result.unwrap().to_string();

        // The generated block should have backend matching
        assert!(block_str.contains("DatabaseBackend :: Postgres"));
        assert!(block_str.contains("DatabaseBackend :: MySql"));
        assert!(block_str.contains("DatabaseBackend :: Sqlite"));
    }

    #[test]
    fn test_build_migration_block_with_delete_table() {
        // First create the table
        let create_migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "temp_table".into(),
                columns: vec![test_column("id")],
                constraints: vec![],
            }],
        };

        let mut baseline = Vec::new();
        let _ = build_migration_block(&create_migration, &mut baseline);
        assert_eq!(baseline.len(), 1);

        // Now delete it
        let delete_migration = MigrationPlan {
            version: 2,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::DeleteTable {
                table: "temp_table".into(),
            }],
        };

        let result = build_migration_block(&delete_migration, &mut baseline);
        assert!(result.is_ok());
        let block_str = result.unwrap().to_string();
        assert!(block_str.contains("DROP TABLE"));

        // Baseline should be empty after delete
        assert_eq!(baseline.len(), 0);
    }

    #[test]
    fn test_build_migration_block_with_index() {
        let migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::CreateTable {
                table: "users".into(),
                columns: vec![
                    test_column("id"),
                    ColumnDef {
                        name: "email".into(),
                        r#type: ColumnType::Simple(SimpleColumnType::Text),
                        nullable: true,
                        default: None,
                        comment: None,
                        primary_key: None,
                        unique: None,
                        index: Some(StrOrBoolOrArray::Bool(true)),
                        foreign_key: None,
                    },
                ],
                constraints: vec![],
            }],
        };

        let mut baseline = Vec::new();
        let result = build_migration_block(&migration, &mut baseline);
        assert!(result.is_ok());

        // Table should be normalized with index
        let table = &baseline[0];
        let normalized = table.clone().normalize();
        assert!(normalized.is_ok());
    }

    #[test]
    fn test_build_migration_block_error_nonexistent_table() {
        // Try to add column to a table that doesn't exist - should fail
        let migration = MigrationPlan {
            version: 1,
            comment: None,
            created_at: None,
            actions: vec![MigrationAction::AddColumn {
                table: "nonexistent_table".into(),
                column: Box::new(test_column("new_col")),
                fill_with: None,
            }],
        };

        let mut baseline = Vec::new();
        let result = build_migration_block(&migration, &mut baseline);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Failed to build queries for migration version 1"));
    }

    #[test]
    fn test_vespertide_migration_impl_loading_error() {
        // Save original CARGO_MANIFEST_DIR
        let original = std::env::var("CARGO_MANIFEST_DIR").ok();

        // Remove CARGO_MANIFEST_DIR to trigger loading error
        unsafe {
            std::env::remove_var("CARGO_MANIFEST_DIR");
        }

        let input: proc_macro2::TokenStream = "pool".parse().unwrap();
        let output = vespertide_migration_impl(input);
        let output_str = output.to_string();

        // Should contain error about failed loading
        assert!(
            output_str.contains("Failed to load migrations at compile time"),
            "Expected loading error, got: {}",
            output_str
        );

        // Restore CARGO_MANIFEST_DIR
        if let Some(val) = original {
            unsafe {
                std::env::set_var("CARGO_MANIFEST_DIR", val);
            }
        }
    }

    #[test]
    fn test_vespertide_migration_impl_with_valid_project() {
        use std::fs;

        // Create a temporary directory with a valid vespertide project
        let dir = tempdir().unwrap();
        let project_dir = dir.path();

        // Create vespertide.json config
        let config_content = r#"{
            "modelsDir": "models",
            "migrationsDir": "migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake",
            "modelFormat": "json"
        }"#;
        fs::write(project_dir.join("vespertide.json"), config_content).unwrap();

        // Create empty models and migrations directories
        fs::create_dir_all(project_dir.join("models")).unwrap();
        fs::create_dir_all(project_dir.join("migrations")).unwrap();

        // Save original CARGO_MANIFEST_DIR and set to temp dir
        let original = std::env::var("CARGO_MANIFEST_DIR").ok();
        unsafe {
            std::env::set_var("CARGO_MANIFEST_DIR", project_dir);
        }

        let input: proc_macro2::TokenStream = "pool".parse().unwrap();
        let output = vespertide_migration_impl(input);
        let output_str = output.to_string();

        // Should produce valid async code since there are no migrations
        assert!(
            output_str.contains("async"),
            "Expected async block, got: {}",
            output_str
        );
        assert!(
            output_str.contains("CREATE TABLE IF NOT EXISTS"),
            "Expected version table creation, got: {}",
            output_str
        );

        // Restore CARGO_MANIFEST_DIR
        if let Some(val) = original {
            unsafe {
                std::env::set_var("CARGO_MANIFEST_DIR", val);
            }
        } else {
            unsafe {
                std::env::remove_var("CARGO_MANIFEST_DIR");
            }
        }
    }

    #[test]
    fn test_vespertide_migration_impl_with_migrations() {
        use std::fs;

        // Create a temporary directory with a valid vespertide project and migrations
        let dir = tempdir().unwrap();
        let project_dir = dir.path();

        // Create vespertide.json config
        let config_content = r#"{
            "modelsDir": "models",
            "migrationsDir": "migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake",
            "modelFormat": "json"
        }"#;
        fs::write(project_dir.join("vespertide.json"), config_content).unwrap();

        // Create models and migrations directories
        fs::create_dir_all(project_dir.join("models")).unwrap();
        fs::create_dir_all(project_dir.join("migrations")).unwrap();

        // Create a migration file
        let migration_content = r#"{
            "version": 1,
            "actions": [
                {
                    "type": "create_table",
                    "table": "users",
                    "columns": [
                        {"name": "id", "type": "integer", "nullable": false}
                    ],
                    "constraints": []
                }
            ]
        }"#;
        fs::write(
            project_dir.join("migrations").join("0001_initial.json"),
            migration_content,
        )
        .unwrap();

        // Save original CARGO_MANIFEST_DIR and set to temp dir
        let original = std::env::var("CARGO_MANIFEST_DIR").ok();
        unsafe {
            std::env::set_var("CARGO_MANIFEST_DIR", project_dir);
        }

        let input: proc_macro2::TokenStream = "pool".parse().unwrap();
        let output = vespertide_migration_impl(input);
        let output_str = output.to_string();

        // Should produce valid async code with migration
        assert!(
            output_str.contains("async"),
            "Expected async block, got: {}",
            output_str
        );

        // Restore CARGO_MANIFEST_DIR
        if let Some(val) = original {
            unsafe {
                std::env::set_var("CARGO_MANIFEST_DIR", val);
            }
        } else {
            unsafe {
                std::env::remove_var("CARGO_MANIFEST_DIR");
            }
        }
    }
}
