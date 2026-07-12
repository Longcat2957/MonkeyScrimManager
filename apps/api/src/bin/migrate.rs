use std::{error::Error, process::ExitCode};

use monkeybot_api::{
    config::MigrationConfig,
    infrastructure::postgres::{self, MIGRATOR, latest_migration_version},
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("monkeybot migration failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = MigrationConfig::from_env()?;
    let pool = postgres::connect_for_migration(&config).await?;
    let result = MIGRATOR.run(&pool).await;
    pool.close().await;
    result?;
    println!(
        "monkeybot migrations applied through version {}",
        latest_migration_version()
    );
    Ok(())
}
