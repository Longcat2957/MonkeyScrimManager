use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use secrecy::SecretString;
use thiserror::Error;

const DEFAULT_PORT: u16 = 30040;
const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_MATCH_COUNT: usize = 10;
pub(crate) const MAX_MATCH_COUNT: usize = 20;

#[derive(Clone)]
pub struct Config {
    bind_addr: SocketAddr,
    api_key: SecretString,
    internal_token: SecretString,
    request_timeout: Duration,
    match_count: usize,
    season_start_unix_seconds: i64,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_map(values: &HashMap<String, String>) -> Result<Self, ConfigError> {
        Self::from_lookup(|key| values.get(key).cloned())
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind_addr = lookup("MONKEYBOT_RIOT_BIND_ADDR")
            .unwrap_or_else(|| {
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT).to_string()
            })
            .parse()
            .map_err(|_| ConfigError::Invalid("MONKEYBOT_RIOT_BIND_ADDR"))?;
        let api_key = required(&mut lookup, "MONKEYBOT_RIOT_API_KEY")?;
        let internal_token = required(&mut lookup, "MONKEYBOT_RIOT_INTERNAL_TOKEN")?;
        if internal_token.len() < 32 {
            return Err(ConfigError::Invalid("MONKEYBOT_RIOT_INTERNAL_TOKEN"));
        }
        let request_timeout_ms = parse_number(
            lookup("MONKEYBOT_RIOT_REQUEST_TIMEOUT_MS"),
            DEFAULT_TIMEOUT_MS,
            "MONKEYBOT_RIOT_REQUEST_TIMEOUT_MS",
        )?;
        if !(500..=30_000).contains(&request_timeout_ms) {
            return Err(ConfigError::Invalid("MONKEYBOT_RIOT_REQUEST_TIMEOUT_MS"));
        }
        let match_count = parse_number(
            lookup("MONKEYBOT_RIOT_MATCH_COUNT"),
            DEFAULT_MATCH_COUNT,
            "MONKEYBOT_RIOT_MATCH_COUNT",
        )?;
        if !(1..=MAX_MATCH_COUNT).contains(&match_count) {
            return Err(ConfigError::Invalid("MONKEYBOT_RIOT_MATCH_COUNT"));
        }
        let season_start_unix_seconds =
            required(&mut lookup, "MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS")?
                .parse::<i64>()
                .map_err(|_| ConfigError::Invalid("MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS"))?;
        if season_start_unix_seconds <= 0 {
            return Err(ConfigError::Invalid(
                "MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS",
            ));
        }

        Ok(Self {
            bind_addr,
            api_key: SecretString::from(api_key),
            internal_token: SecretString::from(internal_token),
            request_timeout: Duration::from_millis(request_timeout_ms),
            match_count,
            season_start_unix_seconds,
        })
    }

    pub const fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub const fn api_key(&self) -> &SecretString {
        &self.api_key
    }

    pub const fn internal_token(&self) -> &SecretString {
        &self.internal_token
    }

    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub const fn match_count(&self) -> usize {
        self.match_count
    }

    pub const fn season_start_unix_seconds(&self) -> i64 {
        self.season_start_unix_seconds
    }
}

fn required(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
) -> Result<String, ConfigError> {
    lookup(key)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing(key))
}

fn parse_number<T: std::str::FromStr>(
    value: Option<String>,
    default: T,
    key: &'static str,
) -> Result<T, ConfigError> {
    match value {
        Some(value) => value.parse().map_err(|_| ConfigError::Invalid(key)),
        None => Ok(default),
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid environment variable {0}")]
    Invalid(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_values() -> HashMap<String, String> {
        HashMap::from([
            ("MONKEYBOT_RIOT_API_KEY".into(), "RGAPI-test".into()),
            (
                "MONKEYBOT_RIOT_INTERNAL_TOKEN".into(),
                "0123456789abcdef0123456789abcdef".into(),
            ),
            (
                "MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS".into(),
                "1735689600".into(),
            ),
        ])
    }

    #[test]
    fn parses_safe_defaults() {
        let config = Config::from_map(&valid_values()).expect("config");
        assert_eq!(config.bind_addr().port(), DEFAULT_PORT);
        assert_eq!(config.match_count(), DEFAULT_MATCH_COUNT);
        assert_eq!(config.request_timeout(), Duration::from_millis(5_000));
        assert_eq!(config.season_start_unix_seconds(), 1_735_689_600);
    }

    #[test]
    fn rejects_short_internal_token() {
        let mut values = valid_values();
        values.insert("MONKEYBOT_RIOT_INTERNAL_TOKEN".into(), "short".into());
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid("MONKEYBOT_RIOT_INTERNAL_TOKEN"))
        ));
    }

    #[test]
    fn match_sample_size_accepts_ten_or_twenty_but_never_exceeds_twenty() {
        for match_count in [10, 20] {
            let mut values = valid_values();
            values.insert("MONKEYBOT_RIOT_MATCH_COUNT".into(), match_count.to_string());
            assert_eq!(
                Config::from_map(&values)
                    .expect("valid sample size")
                    .match_count(),
                match_count
            );
        }

        let mut values = valid_values();
        values.insert("MONKEYBOT_RIOT_MATCH_COUNT".into(), "21".into());
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid("MONKEYBOT_RIOT_MATCH_COUNT"))
        ));
    }

    #[test]
    fn season_start_must_be_an_explicit_positive_unix_timestamp() {
        let mut missing = valid_values();
        missing.remove("MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS");
        assert!(matches!(
            Config::from_map(&missing),
            Err(ConfigError::Missing(
                "MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS"
            ))
        ));

        for value in ["0", "-1", "not-a-timestamp"] {
            let mut values = valid_values();
            values.insert(
                "MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS".into(),
                value.into(),
            );
            assert!(matches!(
                Config::from_map(&values),
                Err(ConfigError::Invalid(
                    "MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS"
                ))
            ));
        }
    }
}
