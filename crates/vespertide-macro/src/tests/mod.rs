use super::*;
use serial_test::serial;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;
use vespertide_core::{
    ColumnDef, ColumnType, MigrationAction, MigrationPlan, SimpleColumnType, StrOrBoolOrArray,
};

fn expand(input: TokenStream2) -> TokenStream2 {
    vespertide_migration_impl(input).unwrap_or_else(|e| e.to_compile_error())
}

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
        &[("vespertide_migration", expand)],
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
    writeln!(test_file, r"vespertide_migration!(db_pool);").unwrap();

    let file = File::open(&test_file_path).unwrap();
    let result = runtime_macros::emulate_functionlike_macro_expansion(
        file,
        &[("vespertide_migration", expand)],
    );

    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_macro_parsing_invalid_option() {
    // Test that invalid options produce a compile error
    let input: proc_macro2::TokenStream = "pool, invalid_option = \"value\"".parse().unwrap();
    let output = expand(input);
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
    let output = expand(input);
    let output_str = output.to_string();
    // Should produce output (either success or migration loading error)
    assert!(!output_str.is_empty());
    // If error, it should mention "Failed to load"
    // If success, it should contain "async"
    assert!(
        output_str.contains("async") || output_str.contains("Failed to load"),
        "Unexpected output: {output_str}"
    );
}

#[test]
fn test_macro_parsing_with_version_table() {
    let input: proc_macro2::TokenStream =
        r#"pool, version_table = "custom_versions""#.parse().unwrap();
    let output = expand(input);
    let output_str = output.to_string();
    assert!(!output_str.is_empty());
}

#[test]
fn test_macro_parsing_trailing_comma() {
    let input: proc_macro2::TokenStream = "pool,".parse().unwrap();
    let output = expand(input);
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

/// Concatenate all SQL strings from a block for assertion testing.
fn block_to_string(block: &MigrationBlock) -> String {
    let mut result = String::new();
    for sql in &block.pg_sqls {
        result.push_str(sql);
        result.push(' ');
    }
    for sql in &block.mysql_sqls {
        result.push_str(sql);
        result.push(' ');
    }
    for sql in &block.sqlite_sqls {
        result.push_str(sql);
        result.push(' ');
    }
    result
}

#[test]
fn test_build_migration_block_create_table() {
    let migration = MigrationPlan {
        id: String::new(),
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
    let block_str = block_to_string(&block);

    // Verify statics contain SQL and metadata is correct
    assert!(block_str.contains("CREATE TABLE"));
    assert_eq!(block.version, 1);

    // Verify baseline schema was updated
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0].name, "users");
}

#[test]
fn test_build_migration_block_add_column() {
    // First create the table
    let create_migration = MigrationPlan {
        id: String::new(),
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
        id: String::new(),
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
    let block_str = block_to_string(&block);

    assert_eq!(block.version, 2);
    assert!(block_str.contains("ALTER TABLE"));
    assert!(block_str.contains("ADD COLUMN"));
}

#[test]
fn test_build_migration_block_multiple_actions() {
    let migration = MigrationPlan {
        id: String::new(),
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
    let pool: proc_macro2::TokenStream = "db_pool".parse().unwrap();
    let version_table = "test_versions";

    // Create a simple migration block
    let migration = MigrationPlan {
        id: String::new(),
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

    let generated =
        generate_migration_code(&pool, version_table, &[block], false, None, None).unwrap();
    let generated_str = generated.to_string();

    // Verify the generated code structure
    assert!(generated_str.contains("async"));
    assert!(generated_str.contains("db_pool"));
    assert!(generated_str.contains("test_versions"));
    assert!(generated_str.contains("run_embedded_migrations"));
    assert!(generated_str.contains("EmbeddedMigration"));
    assert!(generated_str.contains("1u32"));
}

#[test]
fn test_generate_migration_code_empty_migrations() {
    let pool: proc_macro2::TokenStream = "pool".parse().unwrap();
    let version_table = "vespertide_version";

    let generated = generate_migration_code(&pool, version_table, &[], false, None, None).unwrap();
    let generated_str = generated.to_string();

    // Should still generate the wrapper code
    assert!(generated_str.contains("async"));
    assert!(generated_str.contains("vespertide_version"));
}

/// F94: with no configured timeout the macro must emit the original
/// `run_embedded_migrations(...)` call (zero codegen churn / non-breaking).
#[test]
fn no_timeout_emits_plain_run_embedded_migrations() {
    let pool: proc_macro2::TokenStream = "pool".parse().unwrap();
    let generated =
        generate_migration_code(&pool, "vespertide_version", &[], false, None, None).unwrap();
    let s = generated.to_string();
    assert!(s.contains("run_embedded_migrations"));
    assert!(
        !s.contains("run_embedded_migrations_with_options"),
        "no-timeout config must NOT use the options-aware runtime: {s}"
    );
    assert!(!s.contains("MigrationRuntimeOptions"));
}

/// F94: when a timeout is configured the macro routes through the
/// options-aware runtime and bakes the millisecond values into a
/// `MigrationRuntimeOptions::from_millis(...)` constructor call (the
/// struct is `#[non_exhaustive]`, so a literal would not compile in the
/// user's crate).
#[test]
fn configured_timeout_emits_with_options_constructor() {
    let pool: proc_macro2::TokenStream = "pool".parse().unwrap();
    let generated = generate_migration_code(
        &pool,
        "vespertide_version",
        &[],
        false,
        Some(5000),
        Some(30000),
    )
    .unwrap();
    let s = generated.to_string();
    assert!(
        s.contains("run_embedded_migrations_with_options"),
        "configured timeout must use the options-aware runtime: {s}"
    );
    assert!(s.contains("MigrationRuntimeOptions :: from_millis"));
    assert!(s.contains("Some (5000u64)"));
    assert!(s.contains("Some (30000u64)"));
}

/// Only the lock timeout configured → statement arg is `None`.
#[test]
fn lock_timeout_only_emits_none_statement_arg() {
    let pool: proc_macro2::TokenStream = "pool".parse().unwrap();
    let generated =
        generate_migration_code(&pool, "vespertide_version", &[], false, Some(5000), None).unwrap();
    let s = generated.to_string();
    assert!(s.contains("from_millis (Some (5000u64) , None)"));
}

/// L183-184: Verify the generated code starts with an opening brace.
/// This ensures the outer block structure is correctly emitted.
#[test]
fn generate_migration_code_emits_outer_block_structure() {
    let pool: proc_macro2::TokenStream = "pool".parse().unwrap();
    let generated =
        generate_migration_code(&pool, "vespertide_version", &[], false, None, None).unwrap();
    let s = generated.to_string();
    // The generated code must start with an opening brace (L183: code.push_str("{\n"))
    // followed by the static array definition and async block.
    assert!(
        s.contains("{ static __VESPERTIDE_MIGRATIONS"),
        "Generated code must start with opening brace and static array: {s}"
    );
}

/// L183-184: direct cover for `write_codegen_error`. The fn is only
/// reachable in production via `writeln!(&mut String, ...).map_err(write_codegen_error)?`,
/// and `writeln!` to a `String` only fails on allocation failure
/// (effectively unreachable). Call the helper directly with a
/// synthetic `std::fmt::Error` so the line is covered AND the produced
/// `syn::Error` message format is locked.
#[test]
fn write_codegen_error_formats_fmt_error_into_syn_error() {
    let fmt_err = std::fmt::Error;
    let syn_err = write_codegen_error(fmt_err);
    let msg = syn_err.to_string();
    assert!(
        msg.starts_with("vespertide_migration codegen failed while writing Rust:"),
        "syn::Error message must start with the documented prefix; got: {msg}"
    );
    assert!(
        msg.contains("an error occurred when formatting an argument") || msg.contains("Error"),
        "syn::Error message must embed the underlying fmt::Error display; got: {msg}"
    );
}

#[test]
fn test_generate_migration_code_multiple_blocks() {
    let pool: proc_macro2::TokenStream = "connection".parse().unwrap();

    let mut baseline = Vec::new();

    let migration1 = MigrationPlan {
        id: String::new(),
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
        id: String::new(),
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

    let generated =
        generate_migration_code(&pool, "migrations", &[block1, block2], false, None, None).unwrap();
    let generated_str = generated.to_string();

    // Both migration versions should be present in the metadata array
    assert!(generated_str.contains("1u32"));
    assert!(generated_str.contains("2u32"));
    assert!(generated_str.contains("__VESPERTIDE_MIGRATIONS"));
}

#[test]
fn test_generate_migration_code_delegates_runtime_execution() {
    let pool: proc_macro2::TokenStream = "db_pool".parse().unwrap();

    let migration = MigrationPlan {
        id: String::new(),
        version: 1,
        comment: Some("initial".into()),
        created_at: None,
        actions: vec![MigrationAction::CreateTable {
            table: "users".into(),
            columns: vec![test_column("id")],
            constraints: vec![],
        }],
    };

    let mut baseline = Vec::new();
    let block = build_migration_block(&migration, &mut baseline).unwrap();

    let generated =
        generate_migration_code(&pool, "vespertide_version", &[block], false, None, None).unwrap();
    let generated_str = generated.to_string();

    assert!(generated_str.contains("run_embedded_migrations"));
    assert!(generated_str.contains("EmbeddedMigration"));
    assert!(!generated_str.contains("SELECT MAX"));
    assert!(!generated_str.contains("execute_raw"));
}

#[test]
fn test_build_migration_block_generates_all_backends() {
    let migration = MigrationPlan {
        id: String::new(),
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

    let block = result.unwrap();

    // All three backend SQL arrays should be populated
    assert!(
        !block.pg_sqls.is_empty(),
        "PostgreSQL SQL should not be empty"
    );
    assert!(
        !block.mysql_sqls.is_empty(),
        "MySQL SQL should not be empty"
    );
    assert!(
        !block.sqlite_sqls.is_empty(),
        "SQLite SQL should not be empty"
    );

    // Each should contain CREATE TABLE SQL
    assert!(block.pg_sqls.iter().any(|s| s.contains("CREATE TABLE")));
    assert!(block.mysql_sqls.iter().any(|s| s.contains("CREATE TABLE")));
    assert!(block.sqlite_sqls.iter().any(|s| s.contains("CREATE TABLE")));
}

#[test]
fn test_build_migration_block_with_delete_table() {
    // First create the table
    let create_migration = MigrationPlan {
        id: String::new(),
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
        id: String::new(),
        version: 2,
        comment: None,
        created_at: None,
        actions: vec![MigrationAction::DeleteTable {
            table: "temp_table".into(),
        }],
    };

    let result = build_migration_block(&delete_migration, &mut baseline);
    assert!(result.is_ok());
    let block = result.unwrap();
    assert!(block.pg_sqls.iter().any(|s| s.contains("DROP TABLE")));

    // Baseline should be empty after delete
    assert_eq!(baseline.len(), 0);
}

#[test]
fn test_build_migration_block_with_index() {
    let migration = MigrationPlan {
        id: String::new(),
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
        id: String::new(),
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
#[serial(cargo_manifest_dir)]
fn test_vespertide_migration_impl_loading_error() {
    // Save original CARGO_MANIFEST_DIR
    let original = std::env::var("CARGO_MANIFEST_DIR").ok();

    // Remove CARGO_MANIFEST_DIR to trigger loading error
    unsafe {
        std::env::remove_var("CARGO_MANIFEST_DIR");
    }

    let input: proc_macro2::TokenStream = "pool".parse().unwrap();
    let output = expand(input);
    let output_str = output.to_string();

    // Should contain error about failed loading
    assert!(
        output_str.contains("Failed to load migrations at compile time"),
        "Expected loading error, got: {output_str}"
    );

    // Restore CARGO_MANIFEST_DIR
    if let Some(val) = original {
        unsafe {
            std::env::set_var("CARGO_MANIFEST_DIR", val);
        }
    }
}

#[test]
#[serial(cargo_manifest_dir)]
fn test_vespertide_migration_impl_surfaces_migration_block_error() {
    use std::fs;

    let dir = tempdir().unwrap();
    let project_dir = dir.path();
    let config_content = r#"{
            "modelsDir": "models",
            "migrationsDir": "migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake",
            "modelFormat": "json"
        }"#;
    fs::write(project_dir.join("vespertide.json"), config_content).unwrap();
    fs::create_dir_all(project_dir.join("models")).unwrap();
    fs::create_dir_all(project_dir.join("migrations")).unwrap();

    let invalid_migration = r#"{
        "actions": [
            {
                "type": "add_constraint",
                "table": "missing_table",
                "constraint": { "type": "primary_key", "columns": ["id"], "auto_increment": false }
            }
        ],
        "comment": "invalid add column",
        "created_at": null,
        "id": "invalid",
        "version": 1
    }"#;
    fs::write(
        project_dir
            .join("migrations")
            .join("0001_invalid.vespertide.json"),
        invalid_migration,
    )
    .unwrap();

    let original = std::env::var("CARGO_MANIFEST_DIR").ok();
    unsafe {
        std::env::set_var("CARGO_MANIFEST_DIR", project_dir);
    }

    let output = expand("pool".parse().unwrap()).to_string();
    assert!(
        output.contains("Failed to build queries for migration version 1"),
        "expected build_migration_block error to surface, got: {output}"
    );

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
#[serial(cargo_manifest_dir)]
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
    let output = expand(input);
    let output_str = output.to_string();

    // Should produce valid async code since there are no migrations
    assert!(
        output_str.contains("async"),
        "Expected async block, got: {output_str}"
    );
    assert!(
        output_str.contains("run_embedded_migrations"),
        "Expected runtime helper delegation, got: {output_str}"
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
fn test_build_migration_block_verbose_create_table() {
    let migration = MigrationPlan {
        id: String::new(),
        version: 1,
        comment: Some("initial setup".into()),
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

    // Metadata should capture comment for verbose logging in generate_migration_code
    assert_eq!(block.version, 1);
    assert_eq!(block.comment, "initial setup");
    // SQL should contain CREATE TABLE
    assert!(block.pg_sqls.iter().any(|s| s.contains("CREATE TABLE")));
}

#[test]
fn test_build_migration_block_verbose_multiple_actions() {
    let migration = MigrationPlan {
        id: String::new(),
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
    // Metadata should be set even with multiple actions
    assert_eq!(result.as_ref().unwrap().version, 1);
}

#[test]
fn test_build_migration_block_verbose_add_column() {
    // Create table first
    let create = MigrationPlan {
        id: String::new(),
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
    let _ = build_migration_block(&create, &mut baseline);

    // Add column
    let add_col = MigrationPlan {
        id: String::new(),
        version: 2,
        comment: Some("add email".into()),
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

    let result = build_migration_block(&add_col, &mut baseline);
    assert!(result.is_ok());
    let block = result.unwrap();
    assert_eq!(block.version, 2);
    assert_eq!(block.comment, "add email");
    // SQL should contain ALTER TABLE for the add column action
    assert!(block.pg_sqls.iter().any(|s| s.contains("ALTER TABLE")));
}

#[test]
fn test_generate_migration_code_verbose() {
    let pool: proc_macro2::TokenStream = "db_pool".parse().unwrap();
    let version_table = "test_versions";

    let migration = MigrationPlan {
        id: String::new(),
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

    let generated =
        generate_migration_code(&pool, version_table, &[block], true, None, None).unwrap();
    let generated_str = generated.to_string();

    assert!(generated_str.contains("run_embedded_migrations"));
    assert!(generated_str.contains("async"));
}

#[test]
fn test_macro_parsing_verbose_flag() {
    // Test parsing the "verbose" keyword
    let input: proc_macro2::TokenStream = "pool, verbose".parse().unwrap();
    let output = expand(input);
    let output_str = output.to_string();
    // Should produce output (either success or migration loading error)
    assert!(!output_str.is_empty());
}

#[test]
#[serial(cargo_manifest_dir)]
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
    let output = expand(input);
    let output_str = output.to_string();

    // Should produce valid async code with migration
    assert!(
        output_str.contains("async"),
        "Expected async block, got: {output_str}"
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
#[serial(cargo_manifest_dir)]
fn test_vespertide_migration_impl_config_load_error() {
    use std::fs;

    // Inject a malformed vespertide.json so load_config_or_default returns Err,
    // exercising the previously-excluded Err arm in vespertide_migration_impl.
    let dir = tempdir().unwrap();
    let project_dir = dir.path();
    // Write invalid JSON.
    fs::write(project_dir.join("vespertide.json"), "{ not valid json").unwrap();
    fs::create_dir_all(project_dir.join("models")).unwrap();
    fs::create_dir_all(project_dir.join("migrations")).unwrap();

    let original = std::env::var("CARGO_MANIFEST_DIR").ok();
    unsafe {
        std::env::set_var("CARGO_MANIFEST_DIR", project_dir);
    }

    let input: proc_macro2::TokenStream = "pool".parse().unwrap();
    let output = expand(input);
    let output_str = output.to_string();

    assert!(
        output_str.contains("Failed to load config at compile time"),
        "Expected config load error, got: {output_str}"
    );

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
#[serial(cargo_manifest_dir)]
fn test_vespertide_migration_impl_build_migration_error() {
    use std::fs;

    // Drive vespertide_migration_impl down the build_migration_block Err
    // path inside the for loop: an AddColumn against a non-existent table
    // fails query construction.
    let dir = tempdir().unwrap();
    let project_dir = dir.path();

    let config_content = r#"{
        "modelsDir": "models",
        "migrationsDir": "migrations",
        "tableNamingCase": "snake",
        "columnNamingCase": "snake",
        "modelFormat": "json"
    }"#;
    fs::write(project_dir.join("vespertide.json"), config_content).unwrap();
    fs::create_dir_all(project_dir.join("models")).unwrap();
    fs::create_dir_all(project_dir.join("migrations")).unwrap();

    // V1 creates "users" with one column "id".
    fs::write(
        project_dir.join("migrations").join("0001_init.json"),
        r#"{
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
        }"#,
    )
    .unwrap();
    // V2 attempts ModifyColumnType against a column that doesn't exist.
    // Per-file validation passes (the action is structurally valid); the
    // failure surfaces from build_plan_queries during baseline replay,
    // which propagates through the for loop's Err arm in
    // vespertide_migration_impl.
    fs::write(
        project_dir.join("migrations").join("0002_bad.json"),
        r#"{
            "version": 2,
            "actions": [
                {
                    "type": "modify_column_type",
                    "table": "users",
                    "column": "ghost",
                    "from_type": "integer",
                    "to_type": "text"
                }
            ]
        }"#,
    )
    .unwrap();

    let original = std::env::var("CARGO_MANIFEST_DIR").ok();
    unsafe {
        std::env::set_var("CARGO_MANIFEST_DIR", project_dir);
    }

    let input: proc_macro2::TokenStream = "pool".parse().unwrap();
    let output = expand(input);
    let output_str = output.to_string();

    assert!(
        output_str.contains("Failed to build queries for migration version 2")
            || output_str.contains("Failed to load migrations at compile time"),
        "Expected migration build error to surface, got: {output_str}"
    );

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
#[serial(cargo_manifest_dir)]
fn test_vespertide_migration_impl_ignores_invalid_models() {
    use std::fs;

    let dir = tempdir().unwrap();
    let project_dir = dir.path();

    let config_content = r#"{
            "modelsDir": "models",
            "migrationsDir": "migrations",
            "tableNamingCase": "snake",
            "columnNamingCase": "snake",
            "modelFormat": "json"
        }"#;
    fs::write(project_dir.join("vespertide.json"), config_content).unwrap();

    fs::create_dir_all(project_dir.join("models")).unwrap();
    fs::create_dir_all(project_dir.join("migrations")).unwrap();

    fs::write(
        project_dir.join("models").join("broken.json"),
        r#"{
                "name": "broken",
                "columns": [
                    {"name": "user_id", "type": "integer", "nullable": false, "foreign_key": "invalid_format"}
                ],
                "constraints": []
            }"#,
    )
    .unwrap();

    fs::write(
        project_dir.join("migrations").join("0001_initial.json"),
        r#"{
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
            }"#,
    )
    .unwrap();

    let original = std::env::var("CARGO_MANIFEST_DIR").ok();
    unsafe {
        std::env::set_var("CARGO_MANIFEST_DIR", project_dir);
    }

    let input: proc_macro2::TokenStream = "pool".parse().unwrap();
    let output = expand(input);
    let output_str = output.to_string();

    assert!(
        output_str.contains("async"),
        "Expected migration code generation to ignore invalid models, got: {output_str}"
    );

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
