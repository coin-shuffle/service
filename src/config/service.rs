use std::net::SocketAddrV4;

use eyre::Context;

#[derive(serde::Deserialize)]
pub(super) struct Raw {
    address: String,
}

pub struct Config {
    pub address: SocketAddrV4,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        let address = raw
            .address
            .parse::<SocketAddrV4>()
            .context("failed to parse addr")?;
        Ok(Config { address })
    }
}
