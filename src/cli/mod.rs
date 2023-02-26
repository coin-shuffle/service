mod actions;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::config::Config;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the server
    Run,
}

impl Cli {
    pub fn new() -> Self {
        Cli::parse()
    }

    async fn run(self) -> eyre::Result<()> {
        let config = Config::from_file(self.config.unwrap_or_default())?;

        match self.command {
            Command::Run => {
                actions::run_service(config).await?;
            }
        }

        Ok(())
    }
}

pub async fn run() -> eyre::Result<()> {
    Cli::new().run().await
}
