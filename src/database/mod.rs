use coin_shuffle_core::service::storage::Storage;
use sqlx::PgPool;

mod migrations;
pub mod participants;
pub mod queues;
pub mod rooms;
mod utils;

#[derive(Clone)]
pub struct Database {
    inner: PgPool,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;

        Ok(Self { inner: pool })
    }
}

impl Storage for Database {}
