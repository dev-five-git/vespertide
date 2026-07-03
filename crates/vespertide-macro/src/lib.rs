// MigrationOptions and MigrationError are now in vespertide-core
// Test-only env mutation uses `std::env::set_var` / `remove_var`, which became
// `unsafe` in Rust 2024. All sites are serialized via `serial_test::serial`.
#![cfg_attr(
    test,
    expect(
        unsafe_code,
        reason = "serial_test serializes Rust 2024 std::env var setters used by macro tests"
    )
)]
//! Procedural macro [`vespertide_migration!`] for zero-runtime-cost
//! database migrations.
//!
//! Loads JSON/YAML migration files at compile time, emits per-backend SQL
//! as static strings, runs them via a small runtime helper.

use std::env;
use std::fmt::Write;
use std::path::PathBuf;

use proc_macro::TokenStream;
#[cfg(not(test))]
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};
use vespertide_loader::{load_config_or_default, load_migrations_at_compile_time};
use vespertide_planner::apply_action;
use vespertide_query::{DatabaseBackend, build_plan_queries};

struct MacroInput {
    pool: proc_macro2::TokenStream,
    version_table: Option<String>,
    verbose: bool,
}

impl Parse for MacroInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut pool_tokens = Vec::new();
        while !input.is_empty() && !input.peek(Token![,]) {
            pool_tokens.push(input.parse::<proc_macro2::TokenTree>()?);
        }
        let pool: proc_macro2::TokenStream = pool_tokens.into_iter().collect();
        let mut version_table = None;
        let mut verbose = false;

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
            } else if key == "verbose" {
                verbose = true;
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
            verbose,
        })
    }
}

/// Returns the root path of the facade `vespertide` crate.
///
/// `proc-macro-crate` makes `vespertide_migration!` work when downstream crates
/// rename the facade dependency in `Cargo.toml`.
#[cfg(not(test))]
fn vespertide_crate_path() -> syn::Result<TokenStream2> {
    match crate_name("vespertide").map_err(|e| {
        syn::Error::new(
            Span::call_site(),
            format!("vespertide crate not found in Cargo.toml dependencies: {e}"),
        )
    })? {
        FoundCrate::Itself => Ok(quote! { crate }),
        FoundCrate::Name(name) => {
            let ident = Ident::new(&name, Span::call_site());
            Ok(quote! { ::#ident })
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "test stub mirrors the production syn::Result<TokenStream2> helper so macro codegen call sites stay uniform"
)]
fn vespertide_crate_path() -> syn::Result<TokenStream2> {
    Ok(quote! { ::vespertide })
}

/// Generated migration block with raw SQL strings for string-based codegen.
#[derive(Debug)]
pub(crate) struct MigrationBlock {
    /// Migration version number
    pub version: u32,
    /// Migration ID for validation
    pub migration_id: String,
    /// Migration comment (for verbose logging)
    pub comment: String,
    /// `PostgreSQL` SQL statements
    pub pg_sqls: Vec<String>,
    /// `MySQL` SQL statements
    pub mysql_sqls: Vec<String>,
    /// `SQLite` SQL statements
    pub sqlite_sqls: Vec<String>,
}

pub(crate) fn build_migration_block(
    migration: &vespertide_core::MigrationPlan,
    baseline_schema: &mut Vec<vespertide_core::TableDef>,
) -> Result<MigrationBlock, String> {
    let version = migration.version;

    // Use the current baseline schema (from all previous migrations)
    let queries = build_plan_queries(migration, baseline_schema)
        .map_err(|e| format!("Failed to build queries for migration version {version}: {e}"))?;

    // Update baseline schema incrementally by applying each action
    for action in &migration.actions {
        let _ = apply_action(baseline_schema, action);
    }

    // Flatten all SQL into per-backend string arrays
    // perf: pre-count backend SQL statements so macro expansion avoids Vec growth reallocations.
    let mut pg_sqls = Vec::with_capacity(queries.iter().map(|q| q.postgres.len()).sum());
    let mut mysql_sqls = Vec::with_capacity(queries.iter().map(|q| q.mysql.len()).sum());
    let mut sqlite_sqls = Vec::with_capacity(queries.iter().map(|q| q.sqlite.len()).sum());

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

    let comment = migration.comment.as_deref().unwrap_or("").to_string();

    Ok(MigrationBlock {
        version,
        migration_id: migration.id.clone(),
        comment,
        pg_sqls,
        mysql_sqls,
        sqlite_sqls,
    })
}

fn generate_migration_code(
    pool: &TokenStream2,
    version_table: &str,
    migration_blocks: &[MigrationBlock],
    verbose: bool,
    lock_timeout_ms: Option<u64>,
    statement_timeout_ms: Option<u64>,
) -> syn::Result<TokenStream2> {
    // Build entire output as a String, parse once at the end.
    // This avoids per-token IPC overhead from quote! (see rust-lang/rust#65080).
    let pool_str = pool.to_string();
    let krate = vespertide_crate_path()?;
    let krate_str = krate.to_string();
    let mut code = String::with_capacity(1_048_576);

    code.push_str("{\n");

    // Emit compact SQL blobs (outside async block for zero runtime cost).
    for b in migration_blocks {
        write_sql_blob(&mut code, &format!("__V{}_PG", b.version), &b.pg_sqls)?;
        write_sql_blob(&mut code, &format!("__V{}_MYSQL", b.version), &b.mysql_sqls)?;
        write_sql_blob(
            &mut code,
            &format!("__V{}_SQLITE", b.version),
            &b.sqlite_sqls,
        )?;
    }

    writeln!(
        code,
        "static __VESPERTIDE_MIGRATIONS: &[{krate_str}::runtime::EmbeddedMigration] = &["
    )
    .map_err(write_codegen_error)?;
    for b in migration_blocks {
        writeln!(code, "{krate_str}::runtime::EmbeddedMigration::new({}u32, {:?}, {:?}, __V{}_PG, __V{}_MYSQL, __V{}_SQLITE),", b.version, b.migration_id, b.comment, b.version, b.version, b.version).map_err(write_codegen_error)?;
    }
    code.push_str("];\n");

    code.push_str("async {\n");
    writeln!(code, "let __pool = &{pool_str};").map_err(write_codegen_error)?;
    // F94: when a timeout is configured, route through the options-aware
    // runtime so backend-appropriate lock/statement timeouts are applied.
    // Otherwise emit the original call unchanged (zero codegen churn).
    if lock_timeout_ms.is_none() && statement_timeout_ms.is_none() {
        writeln!(code, "{krate_str}::runtime::run_embedded_migrations(__pool, {version_table:?}, {verbose}, __VESPERTIDE_MIGRATIONS).await").map_err(write_codegen_error)?;
    } else {
        let lock = render_opt_u64(lock_timeout_ms);
        let stmt = render_opt_u64(statement_timeout_ms);
        writeln!(code, "{krate_str}::runtime::run_embedded_migrations_with_options(__pool, {version_table:?}, {verbose}, __VESPERTIDE_MIGRATIONS, {krate_str}::runtime::MigrationRuntimeOptions::from_millis({lock}, {stmt})).await").map_err(write_codegen_error)?;
    }
    code.push_str("}\n"); // async block
    code.push_str("}\n"); // outer block

    // Single parse — ONE IPC call to rustc tokenizer
    code.parse::<TokenStream2>().map_err(|e| {
        syn::Error::new(
            Span::call_site(),
            format!("vespertide_migration codegen produced invalid Rust: {e}"),
        )
    })
}

/// Render an `Option<u64>` as Rust source for codegen (`Some(5000u64)` / `None`).
fn render_opt_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "None".to_string(), |v| format!("Some({v}u64)"))
}

fn write_codegen_error(e: std::fmt::Error) -> syn::Error {
    syn::Error::new(
        Span::call_site(),
        format!("vespertide_migration codegen failed while writing Rust: {e}"),
    )
}

/// Write a compact SQL blob declaration into the code string.
fn write_sql_blob(code: &mut String, ident: &str, sqls: &[String]) -> syn::Result<()> {
    let mut blob = String::new();
    for sql in sqls {
        blob.push_str(sql);
        blob.push('\0');
    }

    writeln!(code, "static {ident}: &str = {blob:?};").map_err(write_codegen_error)
}

/// Inner implementation that works with `proc_macro2::TokenStream` for testability.
pub(crate) fn vespertide_migration_impl(input: TokenStream2) -> syn::Result<TokenStream2> {
    let input: MacroInput = syn::parse2(input)?;
    let pool = &input.pool;
    let verbose = input.verbose;

    // Get project root from CARGO_MANIFEST_DIR (same as load_migrations_at_compile_time)
    let project_root = match env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => Some(PathBuf::from(dir)),
        Err(_) => None,
    };

    // Load config to get prefix
    let config = match load_config_or_default(project_root) {
        Ok(config) => config,
        Err(e) => {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Failed to load config at compile time: {e}"),
            ));
        }
    };
    let prefix = config.prefix();

    // Apply prefix to version_table if not explicitly provided
    let version_table = input.version_table.map_or_else(
        || config.apply_prefix("vespertide_version"),
        |vt| config.apply_prefix(&vt),
    );

    // Load migration files and build SQL at compile time
    let migrations = match load_migrations_at_compile_time() {
        Ok(migrations) => migrations,
        Err(e) => {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Failed to load migrations at compile time: {e}"),
            ));
        }
    };
    // Apply prefix to migrations and build SQL using incremental baseline schema
    let mut baseline_schema = Vec::new();
    let mut migration_blocks = Vec::new();

    for migration in &migrations {
        // Apply prefix to migration table names
        let prefixed_migration = migration.clone().with_prefix(prefix);
        match build_migration_block(&prefixed_migration, &mut baseline_schema) {
            Ok(block) => migration_blocks.push(block),
            Err(e) => {
                return Err(syn::Error::new(proc_macro2::Span::call_site(), e));
            }
        }
    }

    generate_migration_code(
        pool,
        &version_table,
        &migration_blocks,
        verbose,
        config.lock_timeout_ms(),
        config.statement_timeout_ms(),
    )
}

/// Zero-runtime migration entry point.
///
/// Irreducible proc-macro shell: the `#[proc_macro]` attribute requires the
/// signature `fn(TokenStream) -> TokenStream` (NOT `TokenStream2`), so this
/// wrapper can only convert types and dispatch to
/// `vespertide_migration_impl` (which is fully unit-tested via
/// `proc_macro2::TokenStream` in `tests/mod.rs`). Proc-macro entry fns
/// cannot run in unit tests (they require the compiler's proc-macro server),
/// so this shell is excluded from coverage.
#[cfg(not(tarpaulin_include))]
#[proc_macro]
pub fn vespertide_migration(input: TokenStream) -> TokenStream {
    let input2 = TokenStream2::from(input);
    match vespertide_migration_impl(input2) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[cfg(test)]
mod tests;
