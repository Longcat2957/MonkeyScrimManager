use std::{collections::HashMap, sync::Arc, time::Duration};

use futures_util::future::BoxFuture;
use sqlx::{PgPool, Row};

use super::MIGRATOR;
use crate::state::{ComponentStatus, DependencyProbe, DependencySnapshot};

#[derive(Clone)]
pub struct PostgresReadinessProbe {
    pool: PgPool,
    timeout: Duration,
    valkey: Option<Arc<redis::Client>>,
}

impl PostgresReadinessProbe {
    pub const fn new(pool: PgPool, timeout: Duration) -> Self {
        Self {
            pool,
            timeout,
            valkey: None,
        }
    }

    pub fn with_valkey(mut self, valkey: Arc<redis::Client>) -> Self {
        self.valkey = Some(valkey);
        self
    }

    async fn check_with_connection(&self) -> Result<bool, sqlx::Error> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("SELECT 1").execute(&mut *connection).await?;
        let rows = match sqlx::query(
            "SELECT version, success, checksum FROM public._sqlx_migrations ORDER BY version",
        )
        .fetch_all(&mut *connection)
        .await
        {
            Ok(rows) => rows,
            Err(_) => return Ok(false),
        };
        let applied = rows
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<i64, _>("version")?,
                    (
                        row.try_get::<bool, _>("success")?,
                        row.try_get::<Vec<u8>, _>("checksum")?,
                    ),
                ))
            })
            .collect::<Result<HashMap<_, _>, sqlx::Error>>();
        let applied = applied?;

        Ok(applied.len() == MIGRATOR.iter().count()
            && MIGRATOR.iter().all(|migration| {
                applied
                    .get(&migration.version)
                    .is_some_and(|(success, checksum)| {
                        *success && checksum.as_slice() == migration.checksum.as_ref()
                    })
            }))
    }

    async fn check_valkey(&self) -> ComponentStatus {
        let Some(client) = &self.valkey else {
            return ComponentStatus::NotConfigured;
        };
        let probe = async {
            let mut connection = client.get_multiplexed_async_connection().await?;
            redis::cmd("PING")
                .query_async::<String>(&mut connection)
                .await
        };
        match tokio::time::timeout(self.timeout, probe).await {
            Ok(Ok(response)) if response == "PONG" => ComponentStatus::Available,
            _ => ComponentStatus::Unavailable,
        }
    }
}

impl DependencyProbe for PostgresReadinessProbe {
    fn check(&self) -> BoxFuture<'_, DependencySnapshot> {
        Box::pin(async move {
            let (postgres, valkey) = tokio::join!(
                tokio::time::timeout(self.timeout, self.check_with_connection()),
                self.check_valkey(),
            );
            match postgres {
                Ok(Ok(true)) => DependencySnapshot {
                    postgres: ComponentStatus::Available,
                    migration: ComponentStatus::Available,
                    valkey,
                },
                Ok(Ok(false)) => DependencySnapshot {
                    postgres: ComponentStatus::Available,
                    migration: ComponentStatus::Unavailable,
                    valkey,
                },
                Ok(Err(_)) | Err(_) => DependencySnapshot {
                    postgres: ComponentStatus::Unavailable,
                    migration: ComponentStatus::Unavailable,
                    valkey,
                },
            }
        })
    }
}
