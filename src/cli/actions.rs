use coin_shuffle_contracts_bindings::utxo;
use coin_shuffle_protos::v1::shuffle_service_server::ShuffleServiceServer;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use tonic::transport::Server;

use crate::{config::Config as Cfg, database::Database, service::Service};

pub(super) async fn run_service(cfg: Cfg) -> eyre::Result<()> {
    let db = Database::connect(cfg.database.url.unwrap().as_str()).await?;

    let contract = utxo::Connector::new(cfg.contract.url, cfg.contract.address);

    let service = Service::new(contract, db, "some_key".to_string());

    TermLogger::init(
        cfg.logger.level,
        Config::default(),
        TerminalMode::Stdout,
        ColorChoice::Auto,
    )
    .unwrap();

    Server::builder()
        .add_service(ShuffleServiceServer::new(service))
        .serve(std::net::SocketAddr::V4(cfg.service.address))
        .await?;

    Ok(())
}
