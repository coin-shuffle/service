use eyre::Context;

#[derive(serde::Deserialize)]
pub(super) struct Raw {
    private_key: String,
}

#[derive(Default)]
pub struct Config {
    pub private_key: String,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        let private_key = raw.private_key;
        Ok(Self { private_key })
    }
}
