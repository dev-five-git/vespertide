use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};

mod commands;
mod utils;
use crate::commands::export::OrmArg;
use commands::{
    cmd_diff, cmd_export, cmd_init, cmd_log, cmd_new, cmd_revision, cmd_sql, cmd_status,
};
use vespertide_config::FileFormat;
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
    },
    /// Show SQL per applied migration (chronological log).
    Log {
        /// Database backend for SQL generation.
        #[arg(short = 'b', long = "backend", value_enum, default_value = "postgres")]
        backend: BackendArg,
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
    },
    /// Initialize vespertide.json with defaults.
    Init,
    /// Export models into ORM-specific code.
    Export {
        /// Target ORM for export.
        #[arg(short = 'o', long = "orm", value_enum, default_value = "seaorm")]
        orm: OrmArg,
        /// Output directory (defaults to config modelsDir or src/models).
        #[arg(short = 'd', long = "export-dir")]
        export_dir: Option<std::path::PathBuf>,
    },
}

#[cfg(not(tarpaulin_include))]
fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Diff) => cmd_diff(),
        Some(Commands::Sql { backend }) => cmd_sql(backend.into()),
        Some(Commands::Log { backend }) => cmd_log(backend.into()),
        Some(Commands::New { name, format }) => cmd_new(name, format),
        Some(Commands::Status) => cmd_status(),
        Some(Commands::Revision { message, fill_with }) => cmd_revision(message, fill_with),
        Some(Commands::Init) => cmd_init(),
        Some(Commands::Export { orm, export_dir }) => cmd_export(orm, export_dir),
        None => {
            // No subcommand: show help and exit successfully.
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}
