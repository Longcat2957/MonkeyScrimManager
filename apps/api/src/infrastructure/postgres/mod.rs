mod pool;
mod readiness;
mod transaction;

use sqlx::migrate::Migrator;

pub use pool::{PostgresError, connect, connect_for_migration};
pub use readiness::PostgresReadinessProbe;
pub use transaction::{
    DatabaseTransaction, DeadlockRetryPolicy, TransactionAttemptError, TransactionRetryError,
    begin, run_with_deadlock_retry,
};

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub fn latest_migration_version() -> i64 {
    MIGRATOR
        .iter()
        .next_back()
        .map_or(0, |migration| migration.version)
}
