use eyre::Context;
use sqlx::postgres::PgPool;

#[derive(Clone)]
pub struct Database {
    connection: PgPool,
}

impl Database {
    pub async fn connect(url: String) -> eyre::Result<Self> {
        let connection = PgPool::connect(url.as_str())
            .await
            .wrap_err("Failed to connect to database")?;

        Ok(Self { connection })
    }
}
