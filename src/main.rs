mod cli;
mod config;
mod database;
mod service;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    cli::run().await
}
