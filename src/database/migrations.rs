use sqlx::migrate::Migrator;

static _MIGRATOR: Migrator = sqlx::migrate!();
