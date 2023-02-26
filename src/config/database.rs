#[derive(serde::Deserialize)]
pub(super) struct Raw {
    in_memory: bool,
    url: Option<String>,
}

#[derive(Default)]
pub struct Config {
    pub in_memory: bool,
    pub url: Option<String>,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        Ok(Self {
            in_memory: raw.in_memory,
            url: raw.url,
        })
    }
}
