use eyre::Context;

#[derive(serde::Deserialize)]
pub(super) struct Raw {
    level: String,
}

pub struct Config {
    pub level: log::Level,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        let level = raw.level.parse().context("Failed to parse log level")?;

        Ok(Self { level })
    }
}
