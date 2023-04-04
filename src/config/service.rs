use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use eyre::Context;

#[derive(serde::Deserialize)]
pub(super) struct Raw {
    address: String,
    min_room_size: usize,
    shuffle_round_deadline: u64,
}

pub struct Config {
    pub address: SocketAddrV4,
    pub min_room_size: usize,
    pub shuffle_round_deadline: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080),
            min_room_size: 3,
            shuffle_round_deadline: Duration::from_secs(120),
        }
    }
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        let address = raw
            .address
            .parse::<SocketAddrV4>()
            .context("failed to parse addr")?;

        let shuffle_round_deadline = Duration::from_secs(raw.shuffle_round_deadline);

        Ok(Config {
            address,
            shuffle_round_deadline,
            min_room_size: raw.min_room_size,
        })
    }
}
