use std::{collections::HashMap, time::Duration};

use futures_util::future::BoxFuture;
use sqlx::{PgPool, Row};

use super::MIGRATOR;
use crate::state::{ComponentStatus, DependencyProbe, DependencySnapshot};

#[derive(Clone)]
pub struct PostgresReadinessProbe {
    pool: PgPool,
    timeout: Duration,
}

impl PostgresReadinessProbe {
    pub const fn new(pool: PgPool, timeout: Duration) -> Self {
        Self { pool, timeout }
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
}

impl DependencyProbe for PostgresReadinessProbe {
    fn check(&self) -> BoxFuture<'_, DependencySnapshot> {
        Box::pin(async move {
            match tokio::time::timeout(self.timeout, self.check_with_connection()).await {
                Ok(Ok(true)) => DependencySnapshot {
                    postgres: ComponentStatus::Available,
                    migration: ComponentStatus::Available,
                    valkey: ComponentStatus::NotConfigured,
                },
                Ok(Ok(false)) => DependencySnapshot {
                    postgres: ComponentStatus::Available,
                    migration: ComponentStatus::Unavailable,
                    valkey: ComponentStatus::NotConfigured,
                },
                Ok(Err(_)) | Err(_) => DependencySnapshot {
                    postgres: ComponentStatus::Unavailable,
                    migration: ComponentStatus::Unavailable,
                    valkey: ComponentStatus::NotConfigured,
                },
            }
        })
    }
}
