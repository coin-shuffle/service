use coin_shuffle_contracts_bindings::utxo;
use coin_shuffle_protos::v1::shuffle_service_server::ShuffleServiceServer;
use ethers_core::utils::hex::ToHex;
use eyre::Context;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use tonic::transport::Server;

use crate::{config::Config as Cfg, service::Protocol};

pub(super) async fn run_service(cfg: Cfg) -> eyre::Result<()> {
    let contract = utxo::Connector::with_priv_key(
        cfg.contract.url.to_string(),
        cfg.contract.address.encode_hex(),
        cfg.signer.private_key,
    )
    .await
    .context("failed to init contract connector")?;

    let service = Protocol::new(
        contract,
        cfg.tokens.sign_key,
        cfg.service.shuffle_round_deadline,
        cfg.service.min_room_size,
    );

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
