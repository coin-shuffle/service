mod database;
mod logger;
mod service;
mod signer;

use eyre::Context;
use std::path::PathBuf;

#[derive(serde::Deserialize)]
struct Raw {
    database: database::Raw,
    logger: logger::Raw,
    service: service::Raw,
    // signer: signer::Raw,
}

#[derive(Default)]
pub struct Config {
    pub database: database::Config,
    pub logger: logger::Config,
    pub service: service::Config,
    // pub signer: signer::Config,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        Ok(Self {
            database: raw.database.try_into()?,
            logger: raw.logger.try_into()?,
            service: raw.service.try_into()?,
            // signer: raw.signer.try_into()?,
        })
    }
}

impl Config {
    pub fn from_file(path: impl Into<PathBuf>) -> eyre::Result<Self> {
        let config = config::Config::builder()
            .add_source(config::File::from(path.into()))
            .build()
            .context("Failed to get config")?;

        let raw: Raw = config
            .try_deserialize()
            .context("Failed to deserialize config")?;

        raw.try_into().context("Failed to convert config")
    }
}
