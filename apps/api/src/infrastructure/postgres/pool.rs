use std::str::FromStr;

use secrecy::ExposeSecret;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use thiserror::Error;

use crate::config::{DatabaseConfig, MigrationConfig};

pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, PostgresError> {
    let connect_options = PgConnectOptions::from_str(config.url().expose_secret())
        .map_err(|_| PostgresError::InvalidOptions)?;
    let statement_timeout = postgres_duration(config.statement_timeout());
    let lock_timeout = postgres_duration(config.lock_timeout());
    let idle_transaction_timeout = postgres_duration(config.idle_transaction_timeout());
    let application_name = config.application_name().to_owned();

    PgPoolOptions::new()
        .max_connections(config.max_connections())
        .min_connections(config.min_connections())
        .acquire_timeout(config.acquire_timeout())
        .idle_timeout(Some(config.idle_timeout()))
        .max_lifetime(Some(config.max_lifetime()))
        .after_connect(move |connection, _metadata| {
            let statement_timeout = statement_timeout.clone();
            let lock_timeout = lock_timeout.clone();
            let idle_transaction_timeout = idle_transaction_timeout.clone();
            let application_name = application_name.clone();
            Box::pin(async move {
                sqlx::query(
                    "SELECT set_config('application_name', $1, false), \
                     set_config('search_path', 'public, pg_catalog', false), \
                     set_config('TimeZone', 'UTC', false), \
                     set_config('statement_timeout', $2, false), \
                     set_config('lock_timeout', $3, false), \
                     set_config('idle_in_transaction_session_timeout', $4, false)",
                )
                .bind(&application_name)
                .bind(&statement_timeout)
                .bind(&lock_timeout)
                .bind(&idle_transaction_timeout)
                .execute(connection)
                .await?;
                Ok(())
            })
        })
        .connect_with(connect_options)
        .await
        .map_err(PostgresError::Connect)
}

pub async fn connect_for_migration(config: &MigrationConfig) -> Result<PgPool, PostgresError> {
    let connect_options = PgConnectOptions::from_str(config.database_url().expose_secret())
        .map_err(|_| PostgresError::InvalidOptions)?;

    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(config.acquire_timeout())
        .connect_with(connect_options)
        .await
        .map_err(PostgresError::Connect)
}

fn postgres_duration(duration: std::time::Duration) -> String {
    format!("{}ms", duration.as_millis())
}

#[derive(Debug, Error)]
pub enum PostgresError {
    #[error("invalid PostgreSQL connection options")]
    InvalidOptions,
    #[error("failed to connect to PostgreSQL")]
    Connect(#[source] sqlx::Error),
}
