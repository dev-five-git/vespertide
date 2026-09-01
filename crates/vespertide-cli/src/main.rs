use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

mod commands;
mod parallel_config;
#[cfg(test)]
mod test_support;
mod utils;
use crate::commands::erd::ErdFormat;
use commands::{
    cmd_diff, cmd_erd_with_filters, cmd_export, cmd_init, cmd_log, cmd_new, cmd_revision, cmd_sql,
    cmd_status,
};
use vespertide_config::FileFormat;
use vespertide_exporter::Orm;
use vespertide_query::DatabaseBackend;

#[derive(Copy, Clone, Debug, ValueEnum)]
enum BackendArg {
    Postgres,
    Mysql,
    Sqlite,
}

impl From<BackendArg> for DatabaseBackend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Postgres => DatabaseBackend::Postgres,
            BackendArg::Mysql => DatabaseBackend::MySql,
            BackendArg::Sqlite => DatabaseBackend::Sqlite,
        }
    }
}

/// vespertide command-line interface.
#[derive(Parser, Debug)]
#[command(name = "vespertide", author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show diff between applied migrations and current models.
    Diff,
    /// Show SQL statements for the pending migration plan.
    Sql {
        /// Database backend for SQL generation.
        #[arg(short = 'b', long = "backend", value_enum, default_value = "postgres")]
        backend: BackendArg,
        /// Wrap the emitted statements in a plan-level `BEGIN;` / `COMMIT;`
        /// transaction (opt-in; for pasting into a manual SQL runner).
        /// Note: `MySQL` DDL implicitly commits, so this is advisory there.
        #[arg(long = "transaction")]
        transaction: bool,
    },
    /// Show SQL per applied migration (chronological log).
    Log {
        /// Database backend for SQL generation.
        #[arg(short = 'b', long = "backend", value_enum, default_value = "postgres")]
        backend: BackendArg,
        /// Wrap each migration's statements in a plan-level `BEGIN;` /
        /// `COMMIT;` transaction (opt-in; mirrors how each migration is
        /// applied at runtime). Note: `MySQL` DDL implicitly commits, so this
        /// is advisory there.
        #[arg(long = "transaction")]
        transaction: bool,
    },
    /// Create a new model file from template.
    New {
        /// Model name (table name).
        name: String,
        /// Output format: json|yaml|yml (default: config modelFormat or json).
        #[arg(short = 'f', long = "format", value_enum)]
        format: Option<FileFormat>,
    },
    /// Show current status.
    Status,
    /// Create a new revision with a message.
    Revision {
        #[arg(short = 'm', long = "message")]
        message: String,
        /// Fill values for NOT NULL columns without defaults.
        /// Format: table.column=value (can be specified multiple times)
        #[arg(long = "fill-with")]
        fill_with: Vec<String>,
        /// Delete rows with NULL values instead of filling.
        /// Format: table.column (can be specified multiple times)
        #[arg(long = "delete-null-rows")]
        delete_null_rows: Vec<String>,
    },
    /// Initialize vespertide.json with defaults.
    Init,
    /// Export models into ORM-specific code.
    Export {
        /// Target ORM for export.
        #[arg(short = 'o', long = "orm", value_enum, default_value = "seaorm")]
        orm: Orm,
        /// Output directory (defaults to config modelsDir or src/models).
        #[arg(short = 'd', long = "export-dir")]
        export_dir: Option<std::path::PathBuf>,
    },
    /// Export schema as ERD diagrams (SVG, Mermaid, Graphviz DOT).
    Erd {
        /// Output format: svg|mermaid|dot.
        #[arg(short = 'f', long = "format", value_enum, default_value = "svg")]
        format: ErdFormat,
        /// Output file path (defaults to stdout if not specified).
        #[arg(short = 'o', long = "output")]
        output: Option<std::path::PathBuf>,
        /// Include only these tables, plus FK-graph neighbors from --depth.
        #[arg(long, value_delimiter = ',')]
        include: Vec<String>,
        /// Exclude these tables after applying --include and --depth.
        #[arg(long, value_delimiter = ',')]
        exclude: Vec<String>,
        /// FK-graph hop distance from --include tables. 0 = include set only.
        #[arg(long, default_value = "0")]
        depth: usize,
    },
}

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Diff) => cmd_diff().await,
        Some(Commands::Sql {
            backend,
            transaction,
        }) => cmd_sql(backend.into(), transaction).await,
        Some(Commands::Log {
            backend,
            transaction,
        }) => cmd_log(backend.into(), transaction).await,
        Some(Commands::New { name, format }) => cmd_new(name, format).await,
        Some(Commands::Status) => cmd_status().await,
        Some(Commands::Revision {
            message,
            fill_with,
            delete_null_rows,
        }) => cmd_revision(message, fill_with, delete_null_rows).await,
        Some(Commands::Init) => cmd_init().await,
        Some(Commands::Export { orm, export_dir }) => cmd_export(orm, export_dir).await,
        Some(Commands::Erd {
            format,
            output,
            include,
            exclude,
            depth,
        }) => cmd_erd_with_filters(format, output, include, exclude, depth).await,
        None => {
            // No subcommand: show help and exit successfully.
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_arg_from_postgres() {
        let backend: DatabaseBackend = BackendArg::Postgres.into();
        assert!(matches!(backend, DatabaseBackend::Postgres));
    }

    #[test]
    fn test_backend_arg_from_mysql() {
        let backend: DatabaseBackend = BackendArg::Mysql.into();
        assert!(matches!(backend, DatabaseBackend::MySql));
    }

    #[test]
    fn test_backend_arg_from_sqlite() {
        let backend: DatabaseBackend = BackendArg::Sqlite.into();
        assert!(matches!(backend, DatabaseBackend::Sqlite));
    }
}
