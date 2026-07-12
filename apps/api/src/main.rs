use std::{error::Error, process::ExitCode};

use monkeybot_api::{bootstrap, config::Config, telemetry};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("monkeybot-api startup failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::from_env()?;
    telemetry::init(&config)?;
    bootstrap::run(config).await?;
    Ok(())
}
