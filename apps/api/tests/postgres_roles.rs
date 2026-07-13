use std::{error::Error, time::Duration};

use monkeybot_api::infrastructure::postgres::MIGRATOR;
use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{CmdWaitFor, ExecCommand, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "18.4-alpine3.23";
const POSTGRES_PASSWORD: &str = "monkeybot-role-admin-password";
const DATABASE: &str = "monkeybot_roles";
const MIGRATION_ROLE: &str = "monkeybot_roles_migrator";
const MIGRATION_PASSWORD: &str = "monkeybot-role-migration-password";
const RUNTIME_ROLE: &str = "monkeybot_roles_runtime";
const RUNTIME_PASSWORD: &str = "monkeybot-role-runtime-password";
const SAME_ROLE: &str = "monkeybot_roles_same";
const SAME_ROLE_DATABASE: &str = "monkeybot_roles_same_database";
const PROVISION_SCRIPT: &str = "/tmp/provision-roles.sql";
const GRANTS_SCRIPT: &str = "/tmp/runtime-grants.sql";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_role_contract() -> TestResult {
    tokio::time::timeout(Duration::from_secs(120), run_postgres_role_contract())
        .await
        .map_err(|_| "PostgreSQL role integration test exceeded 120 seconds")??;
    Ok(())
}

async fn run_postgres_role_contract() -> TestResult {
    let server = TestPostgres::start().await?;
    let admin = server
        .connect("postgres", "postgres", POSTGRES_PASSWORD)
        .await?;

    let same_role = server
        .exec_psql([
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-v",
            &format!("database_name={SAME_ROLE_DATABASE}"),
            "-v",
            &format!("migration_role={SAME_ROLE}"),
            "-v",
            &format!("runtime_role={SAME_ROLE}"),
            "-f",
            PROVISION_SCRIPT,
        ])
        .await?;
    assert_ne!(
        same_role.exit_code, 0,
        "stdout: {}\nstderr: {}",
        same_role.stdout, same_role.stderr
    );
    assert!(
        same_role.stdout.contains("must be different")
            || same_role.stderr.contains("must be different")
    );
    assert!(!database_exists(&admin, SAME_ROLE_DATABASE).await?);
    assert!(!role_exists(&admin, SAME_ROLE).await?);

    let provisioned = server
        .exec_psql([
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-v",
            &format!("database_name={DATABASE}"),
            "-v",
            &format!("migration_role={MIGRATION_ROLE}"),
            "-v",
            &format!("runtime_role={RUNTIME_ROLE}"),
            "-f",
            PROVISION_SCRIPT,
        ])
        .await?;
    assert_eq!(provisioned.exit_code, 0, "{}", provisioned.stderr);

    set_role_password(&admin, MIGRATION_ROLE, MIGRATION_PASSWORD).await?;
    set_role_password(&admin, RUNTIME_ROLE, RUNTIME_PASSWORD).await?;

    let migration = server
        .connect(DATABASE, MIGRATION_ROLE, MIGRATION_PASSWORD)
        .await?;
    MIGRATOR.run(&migration).await?;

    let granted = server
        .exec_psql([
            "-U",
            MIGRATION_ROLE,
            "-d",
            DATABASE,
            "-v",
            &format!("runtime_role={RUNTIME_ROLE}"),
            "-f",
            GRANTS_SCRIPT,
        ])
        .await?;
    assert_eq!(granted.exit_code, 0, "{}", granted.stderr);

    let runtime = server
        .connect(DATABASE, RUNTIME_ROLE, RUNTIME_PASSWORD)
        .await?;
    assert_current_table_contract(&runtime).await?;
    assert_future_table_defaults(&migration, &runtime).await?;

    runtime.close().await;
    migration.close().await;
    admin.close().await;
    drop(server);
    Ok(())
}

async fn assert_current_table_contract(runtime: &PgPool) -> TestResult {
    let mut transaction = runtime.begin().await?;
    sqlx::query(
        "INSERT INTO users (user_id, display_name, created_at, updated_at)
         VALUES ('990000000000000001', 'runtime-role-test', now(), now())",
    )
    .execute(&mut *transaction)
    .await?;
    let display_name = sqlx::query_scalar::<_, String>(
        "SELECT display_name FROM users WHERE user_id = '990000000000000001'",
    )
    .fetch_one(&mut *transaction)
    .await?;
    assert_eq!(display_name, "runtime-role-test");
    sqlx::query(
        "UPDATE users SET display_name = 'runtime-role-updated'
         WHERE user_id = '990000000000000001'",
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM users WHERE user_id = '990000000000000001'")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    let migration_count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM public._sqlx_migrations")
            .fetch_one(runtime)
            .await?;
    assert_eq!(migration_count, MIGRATOR.iter().count() as i64);

    assert_permission_denied(
        runtime,
        "CREATE TABLE public.runtime_forbidden (id integer)",
    )
    .await?;
    assert_permission_denied(runtime, "CREATE SCHEMA runtime_forbidden").await?;
    assert_permission_denied(runtime, "TRUNCATE TABLE public.users").await?;
    assert_permission_denied(
        runtime,
        "INSERT INTO public._sqlx_migrations
         SELECT * FROM public._sqlx_migrations WHERE false",
    )
    .await?;
    assert_permission_denied(
        runtime,
        "UPDATE public._sqlx_migrations SET description = description WHERE false",
    )
    .await?;
    assert_permission_denied(runtime, "DELETE FROM public._sqlx_migrations WHERE false").await?;
    assert_permission_denied(runtime, "TRUNCATE TABLE public._sqlx_migrations").await?;

    for table in [
        "auction_actions",
        "auction_point_ledger",
        "mmr_ledger_entries",
        "admin_audit_log",
    ] {
        assert_permission_denied(
            runtime,
            &format!("UPDATE public.{table} SET id = id WHERE false"),
        )
        .await?;
        assert_permission_denied(runtime, &format!("DELETE FROM public.{table} WHERE false"))
            .await?;
    }
    Ok(())
}

async fn assert_future_table_defaults(migration: &PgPool, runtime: &PgPool) -> TestResult {
    sqlx::query(
        "CREATE TABLE public.future_permission_probe (
            id integer PRIMARY KEY,
            value text NOT NULL
         )",
    )
    .execute(migration)
    .await?;

    sqlx::query("INSERT INTO future_permission_probe (id, value) VALUES (1, 'allowed')")
        .execute(runtime)
        .await?;
    let value =
        sqlx::query_scalar::<_, String>("SELECT value FROM future_permission_probe WHERE id = 1")
            .fetch_one(runtime)
            .await?;
    assert_eq!(value, "allowed");
    assert_permission_denied(
        runtime,
        "UPDATE future_permission_probe SET value = 'denied' WHERE id = 1",
    )
    .await?;
    assert_permission_denied(runtime, "DELETE FROM future_permission_probe WHERE id = 1").await?;
    Ok(())
}

async fn assert_permission_denied(pool: &PgPool, statement: &str) -> TestResult {
    let error = sqlx::query(AssertSqlSafe(statement.to_owned()))
        .execute(pool)
        .await
        .expect_err("runtime operation must be denied");
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("42501")
    );
    Ok(())
}

async fn database_exists(admin: &PgPool, database_name: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
        .bind(database_name)
        .fetch_one(admin)
        .await
}

async fn role_exists(admin: &PgPool, role_name: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
        .bind(role_name)
        .fetch_one(admin)
        .await
}

async fn set_role_password(admin: &PgPool, role: &str, password: &str) -> TestResult {
    assert!(
        role.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    );
    assert!(
        password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    );
    sqlx::query(AssertSqlSafe(format!(
        "ALTER ROLE {role} PASSWORD '{password}'"
    )))
    .execute(admin)
    .await?;
    Ok(())
}

struct ExecOutput {
    exit_code: i64,
    stdout: String,
    stderr: String,
}

struct TestPostgres {
    container: ContainerAsync<GenericImage>,
    host: String,
    port: u16,
}

impl TestPostgres {
    async fn start() -> TestResult<Self> {
        let container = GenericImage::new(POSTGRES_IMAGE, POSTGRES_TAG)
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
            .with_env_var("POSTGRES_DB", "postgres")
            .with_copy_to(
                PROVISION_SCRIPT,
                include_bytes!("../../../deploy/postgres/provision-roles.sql").to_vec(),
            )
            .with_copy_to(
                GRANTS_SCRIPT,
                include_bytes!("../../../deploy/postgres/runtime-grants.sql").to_vec(),
            )
            .start()
            .await?;
        let host = container.get_host().await?.to_string();
        let port = container.get_host_port_ipv4(5432.tcp()).await?;
        Ok(Self {
            container,
            host,
            port,
        })
    }

    async fn connect(&self, database: &str, user: &str, password: &str) -> TestResult<PgPool> {
        let url = format!(
            "postgresql://{user}:{password}@{}:{}/{database}",
            self.host, self.port
        );
        Ok(PgPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await?)
    }

    async fn exec_psql<const N: usize>(&self, args: [&str; N]) -> TestResult<ExecOutput> {
        let mut command = vec!["psql", "--no-psqlrc", "-v", "ON_ERROR_STOP=1"];
        command.extend(args);
        let mut result = self
            .container
            .exec(ExecCommand::new(command).with_cmd_ready_condition(CmdWaitFor::exit()))
            .await?;
        let stdout = String::from_utf8(result.stdout_to_vec().await?)?;
        let stderr = String::from_utf8(result.stderr_to_vec().await?)?;
        let exit_code = result
            .exit_code()
            .await?
            .ok_or("psql exec did not report an exit code")?;
        if exit_code != 0 {
            assert!(stdout.is_empty() || !stdout.contains("postgresql://"));
        }
        Ok(ExecOutput {
            exit_code,
            stdout,
            stderr,
        })
    }
}
