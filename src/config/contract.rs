use std::str::FromStr;

use ethers_core::abi::Address;
use eyre::Context;

#[derive(serde::Deserialize)]
pub(super) struct Raw {
    url: String,
    address: String,
}

pub struct Config {
    pub url: url::Url,
    pub address: Address,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: url::Url::parse("http://localhost:8545").unwrap(),
            address: Address::default(),
        }
    }
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        let url = url::Url::parse(&raw.url)
            .wrap_err_with(|| format!("failed to parse URL: {}", raw.url))?;
        let address = Address::from_str(&raw.address)
            .wrap_err_with(|| format!("failed to parse address: {}", raw.address))?;

        Ok(Self { url, address })
    }
}
