mod cli;
mod domain;
mod engine;
mod error;
mod linear;
mod state;
mod tools;
mod tracing_setup;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    tracing_setup::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Run { ticket } => {
            tracing::info!(ticket, "run subcommand invoked (stub)");
        }
    }
    Ok(())
}
