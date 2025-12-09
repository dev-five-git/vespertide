use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
use commands::{cmd_diff, cmd_init, cmd_revision, cmd_status};

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
        Commands::Status => cmd_status(),
        Commands::Revision { message } => cmd_revision(message),
        Commands::Init => cmd_init(),
    }
}
