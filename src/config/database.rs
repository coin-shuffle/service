#[derive(serde::Deserialize)]
pub(super) struct Raw {
    in_memory: bool,
    _url: String,
}

pub struct Config {
    pub in_memory: bool,
    pub url: Option<url::Url>,
}

impl TryFrom<Raw> for Config {
    type Error = eyre::Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        if raw.in_memory {
            Ok(Self {
                in_memory: true,
                url: None,
            })
        } else {
            panic!("External database not supported yet");
            // let url = url::Url::parse(&raw.url).context("Failed to parse database URL")?;
            // Ok(Self {
            //     in_memory: false,
            //     url: Some(url),
            // })
        }
    }
}
