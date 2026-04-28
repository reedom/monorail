use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "monorail", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run { ticket: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_run_subcommand() {
        let cli = Cli::try_parse_from(["monorail", "run", "ACM-123"]).unwrap();
        match cli.command {
            Command::Run { ticket } => assert_eq!(ticket, "ACM-123"),
        }
    }

    #[test]
    fn rejects_missing_ticket() {
        let err = Cli::try_parse_from(["monorail", "run"]).unwrap_err();
        let s = err.to_string();
        assert!(s.contains("required"), "got: {s}");
    }
}
