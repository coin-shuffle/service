mod cli;
mod config;
mod service;
mod waiter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    cli::run().await
}
