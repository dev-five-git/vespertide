use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod utils;
use commands::{cmd_diff, cmd_init, cmd_log, cmd_new, cmd_revision, cmd_sql, cmd_status};
use clap::ValueEnum;

/// vespertide command-line interface.
#[derive(Parser, Debug)]
#[command(name = "vespertide", author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        /// Output format: json|yaml|yml (default: json).
        #[arg(short = 'f', long = "format", default_value = "json", value_enum)]
        format: ModelFormat,
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
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ModelFormat {
    Json,
    Yaml,
    Yml,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Diff => cmd_diff(),
        Commands::Sql => cmd_sql(),
        Commands::Log => cmd_log(),
        Commands::New { name, format } => cmd_new(name, format),
        Commands::Status => cmd_status(),
        Commands::Revision { message } => cmd_revision(message),
        Commands::Init => cmd_init(),
    }
}
