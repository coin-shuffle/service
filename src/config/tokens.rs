#[derive(serde::Deserialize)]
pub(super) struct Raw {
    sign_key: String,
}

#[derive(Default)]
pub struct Config {
    pub sign_key: String,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        Ok(Self {
            sign_key: raw.sign_key,
        })
    }
}
