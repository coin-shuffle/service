use eyre::Context;

#[derive(serde::Deserialize)]
pub(super) struct Raw {
    private_key: String,
}

pub struct Config {
    pub private_key: ethers_signers::LocalWallet,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        let private_key = raw
            .private_key
            .parse()
            .context("failed to parse private key")?;
        Ok(Self { private_key })
    }
}
