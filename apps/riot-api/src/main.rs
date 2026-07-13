use monkeybot_riot_api::{build_app, config::Config};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("monkeybot_riot_api=info")),
        )
        .json()
        .init();

    let config = Config::from_env()?;
    let bind_addr = config.bind_addr();
    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "monkeybot-riot-api listening");
    axum::serve(listener, build_app(config)).await?;
    Ok(())
}
