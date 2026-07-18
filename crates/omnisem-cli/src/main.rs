//! Omni-Sem CLI foundation.

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "omnisem",
    version,
    about = "Private, source-grounded local context for AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Prints foundation status; indexing is intentionally not implemented yet.
    Status,
}

fn main() {
    match Cli::parse().command {
        Some(Command::Status) => {
            println!("Omni-Sem foundation is ready; no index has been configured.");
        }
        None => println!("Run `omnisem --help` for available commands."),
    }
}
