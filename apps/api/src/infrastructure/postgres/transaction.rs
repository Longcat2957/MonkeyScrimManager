use std::{error::Error as StdError, num::NonZeroU32, time::Duration};

use futures_util::future::BoxFuture;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;

pub type DatabaseTransaction = Transaction<'static, Postgres>;

const DEADLOCK_SQLSTATE: &str = "40P01";

pub async fn begin(pool: &PgPool) -> Result<DatabaseTransaction, sqlx::Error> {
    pool.begin().await
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeadlockRetryPolicy {
    max_attempts: NonZeroU32,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl DeadlockRetryPolicy {
    pub const DEFAULT: Self = Self {
        max_attempts: NonZeroU32::new(3).expect("default attempt count is non-zero"),
        initial_backoff: Duration::from_millis(25),
        max_backoff: Duration::from_millis(100),
    };

    pub const fn new(
        max_attempts: NonZeroU32,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Self {
        Self {
            max_attempts,
            initial_backoff,
            max_backoff,
        }
    }

    pub const fn max_attempts(self) -> NonZeroU32 {
        self.max_attempts
    }

    pub const fn initial_backoff(self) -> Duration {
        self.initial_backoff
    }

    pub const fn max_backoff(self) -> Duration {
        self.max_backoff
    }

    fn backoff_after(self, failed_attempt: u32) -> Duration {
        let exponent = failed_attempt.saturating_sub(1).min(31);
        let multiplier = 1_u32 << exponent;
        self.initial_backoff
            .checked_mul(multiplier)
            .unwrap_or(self.max_backoff)
            .min(self.max_backoff)
    }
}

impl Default for DeadlockRetryPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Error)]
pub enum TransactionAttemptError<E> {
    #[error("failed to begin PostgreSQL transaction")]
    Begin(#[source] sqlx::Error),
    #[error("transaction operation failed")]
    Operation(#[source] E),
    #[error("failed to commit PostgreSQL transaction")]
    Commit(#[source] sqlx::Error),
}

#[derive(Debug, Error)]
pub enum TransactionRetryError<E>
where
    E: StdError + 'static,
{
    #[error("transaction attempt {attempt} failed with a non-retryable error")]
    NonRetryable {
        attempt: u32,
        #[source]
        failure: TransactionAttemptError<E>,
    },
    #[error("PostgreSQL deadlock retry exhausted after {attempts} attempts")]
    DeadlockRetriesExhausted {
        attempts: u32,
        #[source]
        failure: TransactionAttemptError<E>,
    },
}

/// Runs one complete transaction per attempt and returns a value only after its commit succeeds.
///
/// `operation_error` must expose the underlying SQLx error, when one exists. The helper itself
/// checks for PostgreSQL SQLSTATE `40P01`; callers cannot make other database failures retryable.
/// Keep non-database side effects out of `operation`, or record them through an outbox in the same
/// transaction, because the operation closure may run more than once.
pub async fn run_with_deadlock_retry<T, E, Operation, OperationError>(
    pool: &PgPool,
    policy: DeadlockRetryPolicy,
    operation: Operation,
    operation_error: OperationError,
) -> Result<T, TransactionRetryError<E>>
where
    T: Send,
    E: StdError + Send + Sync + 'static,
    Operation: for<'transaction> FnMut(
            &'transaction mut DatabaseTransaction,
        ) -> BoxFuture<'transaction, Result<T, E>>
        + Send,
    OperationError: for<'error> Fn(&'error E) -> Option<&'error sqlx::Error> + Send + Sync,
{
    struct AttemptState<'pool, Operation> {
        pool: &'pool PgPool,
        operation: Operation,
    }

    let mut state = AttemptState { pool, operation };

    run_retry_loop(
        &mut state,
        policy,
        |state| {
            Box::pin(async move {
                let mut transaction = state
                    .pool
                    .begin()
                    .await
                    .map_err(TransactionAttemptError::Begin)?;

                let value = match (state.operation)(&mut transaction).await {
                    Ok(value) => value,
                    Err(error) => {
                        // Dropping an uncommitted SQLx transaction schedules a rollback before
                        // the pooled connection can be reused.
                        drop(transaction);
                        return Err(TransactionAttemptError::Operation(error));
                    }
                };

                transaction
                    .commit()
                    .await
                    .map_err(TransactionAttemptError::Commit)?;
                Ok(value)
            })
        },
        |failure| is_deadlock(failure, &operation_error),
    )
    .await
}

async fn run_retry_loop<T, E, State, Attempt, Retryable>(
    state: &mut State,
    policy: DeadlockRetryPolicy,
    mut run_attempt: Attempt,
    retryable: Retryable,
) -> Result<T, TransactionRetryError<E>>
where
    T: Send,
    E: StdError + Send + Sync + 'static,
    State: Send,
    Attempt: for<'attempt> FnMut(
            &'attempt mut State,
        ) -> BoxFuture<'attempt, Result<T, TransactionAttemptError<E>>>
        + Send,
    Retryable: Fn(&TransactionAttemptError<E>) -> bool + Send + Sync,
{
    let max_attempts = policy.max_attempts.get();

    for attempt in 1..=max_attempts {
        let failure = match run_attempt(state).await {
            Ok(value) => return Ok(value),
            Err(failure) => failure,
        };

        if !retryable(&failure) {
            return Err(TransactionRetryError::NonRetryable { attempt, failure });
        }

        if attempt == max_attempts {
            return Err(TransactionRetryError::DeadlockRetriesExhausted {
                attempts: attempt,
                failure,
            });
        }

        drop(failure);
        let backoff = policy.backoff_after(attempt);
        if !backoff.is_zero() {
            tokio::time::sleep(backoff).await;
        }
    }

    unreachable!("a non-zero maximum attempt count always returns from the retry loop")
}

fn is_deadlock<E, OperationError>(
    failure: &TransactionAttemptError<E>,
    operation_error: &OperationError,
) -> bool
where
    OperationError: for<'error> Fn(&'error E) -> Option<&'error sqlx::Error>,
{
    let error = match failure {
        TransactionAttemptError::Begin(error) | TransactionAttemptError::Commit(error) => error,
        TransactionAttemptError::Operation(error) => {
            let Some(error) = operation_error(error) else {
                return false;
            };
            error
        }
    };

    matches!(
        error,
        sqlx::Error::Database(database_error)
            if database_error.code().as_deref() == Some(DEADLOCK_SQLSTATE)
    )
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, fmt};

    use sqlx::error::{DatabaseError, ErrorKind};

    use super::*;

    #[derive(Debug)]
    struct TestDatabaseError {
        code: &'static str,
    }

    impl fmt::Display for TestDatabaseError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("test database error")
        }
    }

    impl StdError for TestDatabaseError {}

    impl DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            "test database error"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.code))
        }

        fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    fn database_error(code: &'static str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(TestDatabaseError { code }))
    }

    #[derive(Debug, Error)]
    #[error("test operation failed")]
    struct TestOperationError(#[source] sqlx::Error);

    fn operation_error(code: &'static str) -> TestOperationError {
        TestOperationError(database_error(code))
    }

    fn expose_sqlx_error(error: &TestOperationError) -> Option<&sqlx::Error> {
        Some(&error.0)
    }

    fn zero_backoff_policy(max_attempts: u32) -> DeadlockRetryPolicy {
        DeadlockRetryPolicy::new(
            NonZeroU32::new(max_attempts).expect("test attempt count is non-zero"),
            Duration::ZERO,
            Duration::ZERO,
        )
    }

    #[tokio::test]
    async fn retries_first_deadlock_then_returns_committed_result() {
        let mut attempts = 0;

        let result = run_retry_loop(
            &mut attempts,
            zero_backoff_policy(3),
            |attempts| {
                Box::pin(async move {
                    *attempts += 1;
                    if *attempts == 1 {
                        Err(TransactionAttemptError::Operation(operation_error(
                            DEADLOCK_SQLSTATE,
                        )))
                    } else {
                        Ok("committed")
                    }
                })
            },
            |failure| is_deadlock(failure, &expose_sqlx_error),
        )
        .await;

        assert_eq!(result.expect("second attempt succeeds"), "committed");
        assert_eq!(attempts, 2);
    }

    #[tokio::test]
    async fn returns_non_deadlock_without_retrying() {
        let mut attempts = 0;

        let error = run_retry_loop(
            &mut attempts,
            zero_backoff_policy(3),
            |attempts| {
                Box::pin(async move {
                    *attempts += 1;
                    Err::<(), _>(TransactionAttemptError::Operation(operation_error("23505")))
                })
            },
            |failure| is_deadlock(failure, &expose_sqlx_error),
        )
        .await
        .expect_err("non-deadlock must fail immediately");

        assert!(matches!(
            error,
            TransactionRetryError::NonRetryable { attempt: 1, .. }
        ));
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn returns_exhaustion_after_bounded_deadlock_attempts() {
        let mut attempts = 0;

        let error = run_retry_loop(
            &mut attempts,
            zero_backoff_policy(3),
            |attempts| {
                Box::pin(async move {
                    *attempts += 1;
                    Err::<(), _>(TransactionAttemptError::Operation(operation_error(
                        DEADLOCK_SQLSTATE,
                    )))
                })
            },
            |failure| is_deadlock(failure, &expose_sqlx_error),
        )
        .await
        .expect_err("deadlocks must stop at the configured bound");

        assert!(matches!(
            error,
            TransactionRetryError::DeadlockRetriesExhausted { attempts: 3, .. }
        ));
        assert_eq!(attempts, 3);
    }
}
