use std::error::Error;

use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;

pub fn init(config: &Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let filter = EnvFilter::try_new(config.log_filter())?;
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(false),
        )
        .try_init()?;
    install_redacted_panic_hook();
    Ok(())
}

fn install_redacted_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        if let Some(location) = panic_info.location() {
            tracing::error!(
                file = location.file(),
                line = location.line(),
                "panic captured"
            );
        } else {
            tracing::error!("panic captured");
        }
    }));
}
