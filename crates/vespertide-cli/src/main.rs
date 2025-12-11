use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

mod commands;
mod utils;
use commands::{cmd_diff, cmd_export, cmd_init, cmd_log, cmd_new, cmd_revision, cmd_sql, cmd_status};
use vespertide_config::FileFormat;
use crate::commands::export::OrmArg;

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
    Sql,
    /// Show SQL per applied migration (chronological log).
    Log,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Diff) => cmd_diff(),
        Some(Commands::Sql) => cmd_sql(),
        Some(Commands::Log) => cmd_log(),
        Some(Commands::New { name, format }) => cmd_new(name, format),
        Some(Commands::Status) => cmd_status(),
        Some(Commands::Revision { message }) => cmd_revision(message),
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
