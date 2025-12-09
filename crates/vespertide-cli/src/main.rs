use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod utils;
use commands::{cmd_diff, cmd_init, cmd_log, cmd_revision, cmd_sql, cmd_status};

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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Diff => cmd_diff(),
        Commands::Sql => cmd_sql(),
        Commands::Log => cmd_log(),
        Commands::Status => cmd_status(),
        Commands::Revision { message } => cmd_revision(message),
        Commands::Init => cmd_init(),
    }
}
