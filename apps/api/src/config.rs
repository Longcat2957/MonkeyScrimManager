use std::{
    collections::{HashMap, HashSet},
    env, fmt,
    net::SocketAddr,
    str::FromStr,
    time::Duration,
};

use axum::http::Uri;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use secrecy::{ExposeSecret, SecretSlice, SecretString, zeroize::Zeroize};
use sqlx::postgres::PgConnectOptions;
use thiserror::Error;
use tracing_subscriber::EnvFilter;

use crate::domain::DiscordUserId;

const DEFAULT_LOG_FILTER: &str = "monkeybot_api=info";
const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 20_000;
const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;
const DEFAULT_DATABASE_MAX_CONNECTIONS: u64 = 10;
const DEFAULT_DATABASE_MIN_CONNECTIONS: u64 = 1;
const DEFAULT_DATABASE_ACQUIRE_TIMEOUT_MS: u64 = 1_000;
const DEFAULT_DATABASE_PROBE_TIMEOUT_MS: u64 = 1_000;
const DEFAULT_DATABASE_STATEMENT_TIMEOUT_MS: u64 = 3_000;
const DEFAULT_DATABASE_LOCK_TIMEOUT_MS: u64 = 1_000;
const DEFAULT_DATABASE_IDLE_TRANSACTION_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_DATABASE_IDLE_TIMEOUT_MS: u64 = 600_000;
const DEFAULT_DATABASE_MAX_LIFETIME_MS: u64 = 1_800_000;
const DEFAULT_SESSION_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const MIN_SESSION_TTL_SECONDS: u64 = 5 * 60;
const MAX_SESSION_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
const MAX_TIMEOUT_MS: u64 = 120_000;
const MAX_DATABASE_CONNECTIONS: u64 = 100;
const MAX_DATABASE_LIFETIME_MS: u64 = 86_400_000;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const MIN_BOT_SERVICE_TOKEN_LENGTH: usize = 32;
const MIN_RIOT_SERVICE_TOKEN_LENGTH: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    Local,
    Development,
    Production,
    Test,
}

impl Environment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Development => "development",
            Self::Production => "production",
            Self::Test => "test",
        }
    }
}

impl FromStr for Environment {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "local" => Ok(Self::Local),
            "development" | "dev" => Ok(Self::Development),
            "production" | "main" => Ok(Self::Production),
            "test" => Ok(Self::Test),
            _ => Err(()),
        }
    }
}

#[derive(Clone)]
pub struct Config {
    environment: Environment,
    dummy_users_enabled: bool,
    bind_addr: SocketAddr,
    public_origin: Uri,
    log_filter: String,
    request_timeout: Duration,
    shutdown_timeout: Duration,
    max_body_bytes: usize,
    database: Option<DatabaseConfig>,
    auth: Option<AuthConfig>,
    bot_service: Option<BotServiceConfig>,
    riot_service: Option<RiotServiceConfig>,
    valkey: Option<ValkeyConfig>,
}

#[derive(Clone)]
pub struct AuthConfig {
    discord_client_id: String,
    discord_client_secret: SecretString,
    oauth_encryption_key: SecretSlice<u8>,
    service_admin_ids: HashSet<DiscordUserId>,
    session_ttl: Duration,
    secure_cookies: bool,
}

#[derive(Clone)]
pub struct BotServiceConfig {
    token: SecretString,
}

#[derive(Clone)]
pub struct RiotServiceConfig {
    base_url: Url,
    token: SecretString,
}

#[derive(Clone)]
pub struct ValkeyConfig {
    url: SecretString,
}

#[derive(Clone)]
pub struct DatabaseConfig {
    url: SecretString,
    application_name: String,
    max_connections: u32,
    min_connections: u32,
    acquire_timeout: Duration,
    probe_timeout: Duration,
    statement_timeout: Duration,
    lock_timeout: Duration,
    idle_transaction_timeout: Duration,
    idle_timeout: Duration,
    max_lifetime: Duration,
}

#[derive(Clone)]
pub struct MigrationConfig {
    database_url: SecretString,
    acquire_timeout: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    pub fn from_map(values: &HashMap<String, String>) -> Result<Self, ConfigError> {
        Self::from_lookup(|key| values.get(key).cloned())
    }

    pub fn from_lookup(
        mut lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let environment = required(&mut lookup, "MONKEYBOT_ENV")?
            .parse()
            .map_err(|()| ConfigError::invalid("MONKEYBOT_ENV", "unsupported environment"))?;
        let dummy_users_enabled = parse_bool(&mut lookup, "MONKEYBOT_DUMMY_USERS_ENABLED", false)?;
        if dummy_users_enabled && environment == Environment::Production {
            return Err(ConfigError::invalid(
                "MONKEYBOT_DUMMY_USERS_ENABLED",
                "cannot be enabled in production",
            ));
        }

        let bind_addr: SocketAddr = required(&mut lookup, "MONKEYBOT_BIND_ADDR")?
            .parse()
            .map_err(|_| ConfigError::invalid("MONKEYBOT_BIND_ADDR", "invalid socket address"))?;
        if bind_addr.port() == 0 {
            return Err(ConfigError::invalid(
                "MONKEYBOT_BIND_ADDR",
                "port must be non-zero",
            ));
        }

        let public_origin = parse_origin(
            &required(&mut lookup, "MONKEYBOT_PUBLIC_ORIGIN")?,
            environment,
        )?;

        let log_filter =
            lookup("MONKEYBOT_LOG_FILTER").unwrap_or_else(|| DEFAULT_LOG_FILTER.to_owned());
        EnvFilter::try_new(&log_filter)
            .map_err(|_| ConfigError::invalid("MONKEYBOT_LOG_FILTER", "invalid filter"))?;

        let request_timeout = Duration::from_millis(parse_u64(
            &mut lookup,
            "MONKEYBOT_REQUEST_TIMEOUT_MS",
            DEFAULT_REQUEST_TIMEOUT_MS,
            1,
            MAX_TIMEOUT_MS,
        )?);
        let shutdown_timeout = Duration::from_millis(parse_u64(
            &mut lookup,
            "MONKEYBOT_SHUTDOWN_TIMEOUT_MS",
            DEFAULT_SHUTDOWN_TIMEOUT_MS,
            1,
            MAX_TIMEOUT_MS,
        )?);
        let max_body_bytes = parse_usize(
            &mut lookup,
            "MONKEYBOT_MAX_BODY_BYTES",
            DEFAULT_MAX_BODY_BYTES,
            1,
            MAX_BODY_BYTES,
        )?;
        let database = DatabaseConfig::from_lookup(&mut lookup, environment)?;
        let auth = AuthConfig::from_lookup(&mut lookup, environment)?;
        let bot_service = BotServiceConfig::from_lookup(&mut lookup)?;
        let riot_service = RiotServiceConfig::from_lookup(&mut lookup)?;
        let valkey = ValkeyConfig::from_lookup(&mut lookup)?;

        Ok(Self {
            environment,
            dummy_users_enabled,
            bind_addr,
            public_origin,
            log_filter,
            request_timeout,
            shutdown_timeout,
            max_body_bytes,
            database,
            auth,
            bot_service,
            riot_service,
            valkey,
        })
    }

    pub const fn environment(&self) -> Environment {
        self.environment
    }

    pub const fn dummy_users_enabled(&self) -> bool {
        self.dummy_users_enabled
    }

    pub const fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    pub fn public_origin(&self) -> &Uri {
        &self.public_origin
    }

    pub fn public_origin_header(&self) -> String {
        format!(
            "{}://{}",
            self.public_origin
                .scheme_str()
                .expect("public origin scheme is validated at startup"),
            self.public_origin
                .authority()
                .expect("public origin authority is validated at startup")
        )
    }

    pub fn oauth_callback_url(&self) -> String {
        format!("{}/oauth/callback", self.public_origin_header())
    }

    pub fn log_filter(&self) -> &str {
        &self.log_filter
    }

    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub const fn shutdown_timeout(&self) -> Duration {
        self.shutdown_timeout
    }

    pub const fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }

    pub const fn database(&self) -> Option<&DatabaseConfig> {
        self.database.as_ref()
    }

    pub const fn auth(&self) -> Option<&AuthConfig> {
        self.auth.as_ref()
    }

    pub const fn bot_service(&self) -> Option<&BotServiceConfig> {
        self.bot_service.as_ref()
    }

    pub const fn riot_service(&self) -> Option<&RiotServiceConfig> {
        self.riot_service.as_ref()
    }

    pub const fn valkey(&self) -> Option<&ValkeyConfig> {
        self.valkey.as_ref()
    }
}

impl BotServiceConfig {
    fn from_lookup(
        lookup: &mut impl FnMut(&str) -> Option<String>,
    ) -> Result<Option<Self>, ConfigError> {
        let Some(token) =
            lookup("MONKEYBOT_BOT_SERVICE_TOKEN").filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        if token.chars().count() < MIN_BOT_SERVICE_TOKEN_LENGTH {
            return Err(ConfigError::invalid(
                "MONKEYBOT_BOT_SERVICE_TOKEN",
                "must contain at least 32 characters",
            ));
        }
        Ok(Some(Self {
            token: SecretString::from(token),
        }))
    }

    pub fn token(&self) -> &SecretString {
        &self.token
    }
}

impl RiotServiceConfig {
    fn from_lookup(
        lookup: &mut impl FnMut(&str) -> Option<String>,
    ) -> Result<Option<Self>, ConfigError> {
        let Some(raw_url) =
            lookup("MONKEYBOT_RIOT_SERVICE_URL").filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        let base_url = Url::parse(&raw_url)
            .map_err(|_| ConfigError::invalid("MONKEYBOT_RIOT_SERVICE_URL", "invalid URL"))?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || !base_url.path().ends_with('/')
        {
            return Err(ConfigError::invalid(
                "MONKEYBOT_RIOT_SERVICE_URL",
                "must be an http(s) directory URL without credentials, query, or fragment",
            ));
        }

        let token = lookup("MONKEYBOT_RIOT_SERVICE_TOKEN")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::Missing {
                key: "MONKEYBOT_RIOT_SERVICE_TOKEN",
            })?;
        if token.chars().count() < MIN_RIOT_SERVICE_TOKEN_LENGTH {
            return Err(ConfigError::invalid(
                "MONKEYBOT_RIOT_SERVICE_TOKEN",
                "must contain at least 32 characters",
            ));
        }

        Ok(Some(Self {
            base_url,
            token: SecretString::from(token),
        }))
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn token(&self) -> &SecretString {
        &self.token
    }
}

impl ValkeyConfig {
    fn from_lookup(
        lookup: &mut impl FnMut(&str) -> Option<String>,
    ) -> Result<Option<Self>, ConfigError> {
        let Some(raw_url) = lookup("MONKEYBOT_VALKEY_URL").filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        let url = Url::parse(&raw_url)
            .map_err(|_| ConfigError::invalid("MONKEYBOT_VALKEY_URL", "invalid URL"))?;
        if url.scheme() != "redis"
            || url.host().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ConfigError::invalid(
                "MONKEYBOT_VALKEY_URL",
                "must be a redis URL without query or fragment",
            ));
        }

        Ok(Some(Self {
            url: SecretString::from(raw_url),
        }))
    }

    pub fn url(&self) -> &SecretString {
        &self.url
    }
}

impl fmt::Debug for RiotServiceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RiotServiceConfig")
            .field("base_url", &self.base_url)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for ValkeyConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValkeyConfig")
            .field("url", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for BotServiceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BotServiceConfig")
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl AuthConfig {
    fn from_lookup(
        lookup: &mut impl FnMut(&str) -> Option<String>,
        environment: Environment,
    ) -> Result<Option<Self>, ConfigError> {
        let client_id =
            lookup("MONKEYBOT_DISCORD_CLIENT_ID").filter(|value| !value.trim().is_empty());
        let client_secret =
            lookup("MONKEYBOT_DISCORD_CLIENT_SECRET").filter(|value| !value.trim().is_empty());
        let encryption_key =
            lookup("MONKEYBOT_OAUTH_ENCRYPTION_KEY").filter(|value| !value.trim().is_empty());

        if environment == Environment::Test
            && client_id.is_none()
            && client_secret.is_none()
            && encryption_key.is_none()
        {
            return Ok(None);
        }

        let discord_client_id = client_id.ok_or(ConfigError::Missing {
            key: "MONKEYBOT_DISCORD_CLIENT_ID",
        })?;
        DiscordUserId::parse(discord_client_id.clone()).map_err(|_| {
            ConfigError::invalid("MONKEYBOT_DISCORD_CLIENT_ID", "must be a Discord snowflake")
        })?;
        let discord_client_secret =
            SecretString::from(client_secret.ok_or(ConfigError::Missing {
                key: "MONKEYBOT_DISCORD_CLIENT_SECRET",
            })?);
        let mut encoded_key = encryption_key.ok_or(ConfigError::Missing {
            key: "MONKEYBOT_OAUTH_ENCRYPTION_KEY",
        })?;
        let decoded_key = URL_SAFE_NO_PAD.decode(&encoded_key);
        encoded_key.zeroize();
        let mut decoded_key = decoded_key.map_err(|_| {
            ConfigError::invalid(
                "MONKEYBOT_OAUTH_ENCRYPTION_KEY",
                "must be an unpadded base64url value",
            )
        })?;
        if decoded_key.len() != 32 {
            decoded_key.zeroize();
            return Err(ConfigError::invalid(
                "MONKEYBOT_OAUTH_ENCRYPTION_KEY",
                "must decode to exactly 32 bytes",
            ));
        }
        let oauth_encryption_key: SecretSlice<u8> = decoded_key.into();

        let service_admin_ids =
            parse_service_admin_ids(lookup("MONKEYBOT_SERVICE_ADMIN_IDS").unwrap_or_default())?;
        let session_ttl = Duration::from_secs(parse_u64(
            lookup,
            "MONKEYBOT_SESSION_TTL_SECONDS",
            DEFAULT_SESSION_TTL_SECONDS,
            MIN_SESSION_TTL_SECONDS,
            MAX_SESSION_TTL_SECONDS,
        )?);

        Ok(Some(Self {
            discord_client_id,
            discord_client_secret,
            oauth_encryption_key,
            service_admin_ids,
            session_ttl,
            secure_cookies: matches!(
                environment,
                Environment::Development | Environment::Production
            ),
        }))
    }

    pub fn discord_client_id(&self) -> &str {
        &self.discord_client_id
    }

    pub fn discord_client_secret(&self) -> &SecretString {
        &self.discord_client_secret
    }

    pub fn oauth_encryption_key(&self) -> &[u8; 32] {
        self.oauth_encryption_key
            .expose_secret()
            .try_into()
            .expect("OAuth encryption key length is validated at startup")
    }

    pub const fn session_ttl(&self) -> Duration {
        self.session_ttl
    }

    pub const fn secure_cookies(&self) -> bool {
        self.secure_cookies
    }

    pub fn is_service_admin(&self, user_id: &DiscordUserId) -> bool {
        self.service_admin_ids.contains(user_id)
    }
}

impl fmt::Debug for AuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthConfig")
            .field("discord_client_id", &self.discord_client_id)
            .field("discord_client_secret", &"[REDACTED]")
            .field("oauth_encryption_key", &"[REDACTED]")
            .field("service_admin_ids", &self.service_admin_ids)
            .field("session_ttl", &self.session_ttl)
            .field("secure_cookies", &self.secure_cookies)
            .finish()
    }
}

impl DatabaseConfig {
    fn from_lookup(
        lookup: &mut impl FnMut(&str) -> Option<String>,
        environment: Environment,
    ) -> Result<Option<Self>, ConfigError> {
        let Some(raw_url) =
            lookup("MONKEYBOT_DATABASE_URL").filter(|value| !value.trim().is_empty())
        else {
            return if environment == Environment::Test {
                Ok(None)
            } else {
                Err(ConfigError::Missing {
                    key: "MONKEYBOT_DATABASE_URL",
                })
            };
        };
        validate_database_url(&raw_url, "MONKEYBOT_DATABASE_URL")?;

        let max_connections = parse_u64(
            lookup,
            "MONKEYBOT_DATABASE_MAX_CONNECTIONS",
            DEFAULT_DATABASE_MAX_CONNECTIONS,
            1,
            MAX_DATABASE_CONNECTIONS,
        )? as u32;
        let min_connections = parse_u64(
            lookup,
            "MONKEYBOT_DATABASE_MIN_CONNECTIONS",
            DEFAULT_DATABASE_MIN_CONNECTIONS,
            0,
            MAX_DATABASE_CONNECTIONS,
        )? as u32;
        if min_connections > max_connections {
            return Err(ConfigError::invalid(
                "MONKEYBOT_DATABASE_MIN_CONNECTIONS",
                "must not exceed MONKEYBOT_DATABASE_MAX_CONNECTIONS",
            ));
        }

        Ok(Some(Self {
            url: SecretString::from(raw_url),
            application_name: format!("monkeybot-api-{}", environment.as_str()),
            max_connections,
            min_connections,
            acquire_timeout: parse_duration(
                lookup,
                "MONKEYBOT_DATABASE_ACQUIRE_TIMEOUT_MS",
                DEFAULT_DATABASE_ACQUIRE_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )?,
            probe_timeout: parse_duration(
                lookup,
                "MONKEYBOT_DATABASE_PROBE_TIMEOUT_MS",
                DEFAULT_DATABASE_PROBE_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )?,
            statement_timeout: parse_duration(
                lookup,
                "MONKEYBOT_DATABASE_STATEMENT_TIMEOUT_MS",
                DEFAULT_DATABASE_STATEMENT_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )?,
            lock_timeout: parse_duration(
                lookup,
                "MONKEYBOT_DATABASE_LOCK_TIMEOUT_MS",
                DEFAULT_DATABASE_LOCK_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )?,
            idle_transaction_timeout: parse_duration(
                lookup,
                "MONKEYBOT_DATABASE_IDLE_TRANSACTION_TIMEOUT_MS",
                DEFAULT_DATABASE_IDLE_TRANSACTION_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )?,
            idle_timeout: parse_duration(
                lookup,
                "MONKEYBOT_DATABASE_IDLE_TIMEOUT_MS",
                DEFAULT_DATABASE_IDLE_TIMEOUT_MS,
                MAX_DATABASE_LIFETIME_MS,
            )?,
            max_lifetime: parse_duration(
                lookup,
                "MONKEYBOT_DATABASE_MAX_LIFETIME_MS",
                DEFAULT_DATABASE_MAX_LIFETIME_MS,
                MAX_DATABASE_LIFETIME_MS,
            )?,
        }))
    }

    pub fn url(&self) -> &SecretString {
        &self.url
    }

    pub fn application_name(&self) -> &str {
        &self.application_name
    }

    pub const fn max_connections(&self) -> u32 {
        self.max_connections
    }

    pub const fn min_connections(&self) -> u32 {
        self.min_connections
    }

    pub const fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout
    }

    pub const fn probe_timeout(&self) -> Duration {
        self.probe_timeout
    }

    pub const fn statement_timeout(&self) -> Duration {
        self.statement_timeout
    }

    pub const fn lock_timeout(&self) -> Duration {
        self.lock_timeout
    }

    pub const fn idle_transaction_timeout(&self) -> Duration {
        self.idle_transaction_timeout
    }

    pub const fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }

    pub const fn max_lifetime(&self) -> Duration {
        self.max_lifetime
    }
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseConfig")
            .field("url", &"[REDACTED]")
            .field("application_name", &self.application_name)
            .field("max_connections", &self.max_connections)
            .field("min_connections", &self.min_connections)
            .field("acquire_timeout", &self.acquire_timeout)
            .field("probe_timeout", &self.probe_timeout)
            .field("statement_timeout", &self.statement_timeout)
            .field("lock_timeout", &self.lock_timeout)
            .field("idle_transaction_timeout", &self.idle_transaction_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .field("max_lifetime", &self.max_lifetime)
            .finish()
    }
}

impl MigrationConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    pub fn from_lookup(
        mut lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let raw_url = required(&mut lookup, "MONKEYBOT_MIGRATION_DATABASE_URL")?;
        validate_database_url(&raw_url, "MONKEYBOT_MIGRATION_DATABASE_URL")?;
        let acquire_timeout = parse_duration(
            &mut lookup,
            "MONKEYBOT_DATABASE_ACQUIRE_TIMEOUT_MS",
            DEFAULT_DATABASE_ACQUIRE_TIMEOUT_MS,
            MAX_TIMEOUT_MS,
        )?;

        Ok(Self {
            database_url: SecretString::from(raw_url),
            acquire_timeout,
        })
    }

    pub fn database_url(&self) -> &SecretString {
        &self.database_url
    }

    pub const fn acquire_timeout(&self) -> Duration {
        self.acquire_timeout
    }
}

impl fmt::Debug for MigrationConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MigrationConfig")
            .field("database_url", &"[REDACTED]")
            .field("acquire_timeout", &self.acquire_timeout)
            .finish()
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("environment", &self.environment)
            .field("dummy_users_enabled", &self.dummy_users_enabled)
            .field("bind_addr", &self.bind_addr)
            .field("public_origin", &self.public_origin)
            .field("log_filter", &self.log_filter)
            .field("request_timeout", &self.request_timeout)
            .field("shutdown_timeout", &self.shutdown_timeout)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("database", &self.database)
            .field("auth", &self.auth)
            .field("bot_service", &self.bot_service)
            .field("riot_service", &self.riot_service)
            .field("valkey", &self.valkey)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {key}")]
    Missing { key: &'static str },
    #[error("invalid environment variable {key}: {reason}")]
    Invalid {
        key: &'static str,
        reason: &'static str,
    },
}

impl ConfigError {
    const fn invalid(key: &'static str, reason: &'static str) -> Self {
        Self::Invalid { key, reason }
    }
}

fn required(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
) -> Result<String, ConfigError> {
    lookup(key)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ConfigError::Missing { key })
}

fn parse_origin(value: &str, environment: Environment) -> Result<Uri, ConfigError> {
    let origin: Uri = value
        .parse()
        .map_err(|_| ConfigError::invalid("MONKEYBOT_PUBLIC_ORIGIN", "invalid URI"))?;
    let scheme = origin.scheme_str();
    let valid_scheme = matches!(scheme, Some("http" | "https"));
    let root_only = origin.path() == "/" && origin.query().is_none();

    if !valid_scheme || origin.authority().is_none() || !root_only {
        return Err(ConfigError::invalid(
            "MONKEYBOT_PUBLIC_ORIGIN",
            "must be an HTTP(S) origin without a path, query, or fragment",
        ));
    }
    if environment == Environment::Production && scheme != Some("https") {
        return Err(ConfigError::invalid(
            "MONKEYBOT_PUBLIC_ORIGIN",
            "production origin must use HTTPS",
        ));
    }

    Ok(origin)
}

fn parse_bool(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
    default: bool,
) -> Result<bool, ConfigError> {
    match lookup(key).as_deref() {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        Some(_) => Err(ConfigError::invalid(key, "must be true, false, 1, or 0")),
    }
}

fn validate_database_url(value: &str, key: &'static str) -> Result<(), ConfigError> {
    value
        .parse::<PgConnectOptions>()
        .map(|_| ())
        .map_err(|_| ConfigError::invalid(key, "invalid PostgreSQL connection URL"))
}

fn parse_duration(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
    default_ms: u64,
    maximum_ms: u64,
) -> Result<Duration, ConfigError> {
    Ok(Duration::from_millis(parse_u64(
        lookup, key, default_ms, 1, maximum_ms,
    )?))
}

fn parse_u64(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, ConfigError> {
    let value = lookup(key)
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| ConfigError::invalid(key, "must be an integer"))
        })
        .transpose()?
        .unwrap_or(default);
    if !(minimum..=maximum).contains(&value) {
        return Err(ConfigError::invalid(
            key,
            "value is outside the allowed range",
        ));
    }
    Ok(value)
}

fn parse_usize(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
    default: usize,
    minimum: usize,
    maximum: usize,
) -> Result<usize, ConfigError> {
    let value = lookup(key)
        .map(|raw| {
            raw.parse::<usize>()
                .map_err(|_| ConfigError::invalid(key, "must be an integer"))
        })
        .transpose()?
        .unwrap_or(default);
    if !(minimum..=maximum).contains(&value) {
        return Err(ConfigError::invalid(
            key,
            "value is outside the allowed range",
        ));
    }
    Ok(value)
}

fn parse_service_admin_ids(raw: String) -> Result<HashSet<DiscordUserId>, ConfigError> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            DiscordUserId::parse(value).map_err(|_| {
                ConfigError::invalid(
                    "MONKEYBOT_SERVICE_ADMIN_IDS",
                    "must contain only comma-separated Discord snowflakes",
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_values() -> HashMap<String, String> {
        HashMap::from([
            ("MONKEYBOT_ENV".to_owned(), "test".to_owned()),
            (
                "MONKEYBOT_BIND_ADDR".to_owned(),
                "127.0.0.1:30020".to_owned(),
            ),
            (
                "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
                "http://localhost:5173".to_owned(),
            ),
        ])
    }

    #[test]
    fn parses_valid_config_with_defaults() {
        let config = Config::from_map(&valid_values()).expect("config should parse");

        assert_eq!(config.environment(), Environment::Test);
        assert!(!config.dummy_users_enabled());
        assert_eq!(config.request_timeout(), Duration::from_secs(10));
        assert_eq!(config.shutdown_timeout(), Duration::from_secs(20));
        assert_eq!(config.max_body_bytes(), 1024 * 1024);
        assert!(config.database().is_none());
        assert!(config.bot_service().is_none());
        assert!(config.riot_service().is_none());
        assert!(config.valkey().is_none());
    }

    #[test]
    fn dummy_users_can_only_be_enabled_outside_production() {
        let mut values = valid_values();
        values.insert(
            "MONKEYBOT_DUMMY_USERS_ENABLED".to_owned(),
            "true".to_owned(),
        );
        assert!(
            Config::from_map(&values)
                .expect("test dummy users should be allowed")
                .dummy_users_enabled()
        );

        values.insert("MONKEYBOT_ENV".to_owned(), "production".to_owned());
        values.insert(
            "MONKEYBOT_DATABASE_URL".to_owned(),
            "postgresql://localhost/monkeybot".to_owned(),
        );
        values.insert(
            "MONKEYBOT_PUBLIC_ORIGIN".to_owned(),
            "https://monkeybot.lol".to_owned(),
        );
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid {
                key: "MONKEYBOT_DUMMY_USERS_ENABLED",
                ..
            })
        ));
    }

    #[test]
    fn parses_optional_bot_service_token_and_redacts_it() {
        let mut values = valid_values();
        let token = "bot-service-token-that-is-at-least-32-characters";
        values.insert("MONKEYBOT_BOT_SERVICE_TOKEN".to_owned(), token.to_owned());

        let config = Config::from_map(&values).expect("config should parse");
        let bot_service = config.bot_service().expect("bot service config");

        assert_eq!(bot_service.token().expose_secret(), token);
        assert!(!format!("{config:?}").contains(token));
        assert!(!format!("{bot_service:?}").contains(token));
    }

    #[test]
    fn rejects_short_bot_service_token() {
        let mut values = valid_values();
        values.insert(
            "MONKEYBOT_BOT_SERVICE_TOKEN".to_owned(),
            "short-service-token".to_owned(),
        );

        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid {
                key: "MONKEYBOT_BOT_SERVICE_TOKEN",
                ..
            })
        ));
    }

    #[test]
    fn riot_service_requires_a_directory_url_and_redacted_token() {
        let mut values = valid_values();
        let token = "riot-service-token-that-is-at-least-32-characters";
        values.insert(
            "MONKEYBOT_RIOT_SERVICE_URL".to_owned(),
            "http://riot-api:8080/internal/riot/".to_owned(),
        );
        values.insert("MONKEYBOT_RIOT_SERVICE_TOKEN".to_owned(), token.to_owned());

        let config = Config::from_map(&values).expect("Riot service config should parse");
        let riot = config.riot_service().expect("Riot service config");
        assert_eq!(riot.base_url().path(), "/internal/riot/");
        assert_eq!(riot.token().expose_secret(), token);
        assert!(!format!("{config:?}").contains(token));

        values.insert(
            "MONKEYBOT_RIOT_SERVICE_URL".to_owned(),
            "http://riot-api:8080/internal/riot".to_owned(),
        );
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid {
                key: "MONKEYBOT_RIOT_SERVICE_URL",
                ..
            })
        ));
    }

    #[test]
    fn riot_service_url_requires_a_service_token() {
        let mut values = valid_values();
        values.insert(
            "MONKEYBOT_RIOT_SERVICE_URL".to_owned(),
            "http://riot-api:8080/internal/riot/".to_owned(),
        );
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Missing {
                key: "MONKEYBOT_RIOT_SERVICE_TOKEN"
            })
        ));
    }

    #[test]
    fn valkey_url_is_optional_validated_and_redacted() {
        let mut values = valid_values();
        let url = "redis://:valkey-password@valkey:6379/";
        values.insert("MONKEYBOT_VALKEY_URL".to_owned(), url.to_owned());

        let config = Config::from_map(&values).expect("Valkey config should parse");
        assert_eq!(
            config
                .valkey()
                .expect("Valkey config")
                .url()
                .expose_secret(),
            url
        );
        assert!(!format!("{config:?}").contains("valkey-password"));

        values.insert(
            "MONKEYBOT_VALKEY_URL".to_owned(),
            "https://valkey:6379/".to_owned(),
        );
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid {
                key: "MONKEYBOT_VALKEY_URL",
                ..
            })
        ));
    }

    #[test]
    fn parses_database_config_without_exposing_the_url() {
        let mut values = valid_values();
        values.insert(
            "MONKEYBOT_DATABASE_URL".to_owned(),
            "postgresql://runtime:database-secret@localhost/monkeybot".to_owned(),
        );

        let config = Config::from_map(&values).expect("config should parse");
        let database = config.database().expect("database config");

        assert_eq!(database.max_connections(), 10);
        assert_eq!(database.min_connections(), 1);
        assert_eq!(database.acquire_timeout(), Duration::from_secs(1));
        assert_eq!(database.probe_timeout(), Duration::from_secs(1));
        assert_eq!(database.statement_timeout(), Duration::from_secs(3));
        assert!(!format!("{config:?}").contains("database-secret"));
    }

    #[test]
    fn parses_auth_config_and_redacts_secrets() {
        let mut values = valid_values();
        values.extend([
            (
                "MONKEYBOT_DISCORD_CLIENT_ID".to_owned(),
                "100000000000000001".to_owned(),
            ),
            (
                "MONKEYBOT_DISCORD_CLIENT_SECRET".to_owned(),
                "discord-client-secret".to_owned(),
            ),
            (
                "MONKEYBOT_OAUTH_ENCRYPTION_KEY".to_owned(),
                URL_SAFE_NO_PAD.encode([7_u8; 32]),
            ),
            (
                "MONKEYBOT_SERVICE_ADMIN_IDS".to_owned(),
                "100000000000000002, 100000000000000003".to_owned(),
            ),
            (
                "MONKEYBOT_SESSION_TTL_SECONDS".to_owned(),
                "3600".to_owned(),
            ),
        ]);

        let config = Config::from_map(&values).expect("config should parse");
        let auth = config.auth().expect("auth config");
        assert_eq!(auth.discord_client_id(), "100000000000000001");
        assert_eq!(auth.session_ttl(), Duration::from_secs(3600));
        assert!(!auth.secure_cookies());
        assert!(
            auth.is_service_admin(
                &DiscordUserId::parse("100000000000000002").expect("valid user ID")
            )
        );
        let output = format!("{config:?}");
        assert!(!output.contains("discord-client-secret"));
        assert!(!output.contains(&URL_SAFE_NO_PAD.encode([7_u8; 32])));
    }

    #[test]
    fn rejects_partial_or_invalid_auth_config() {
        let mut values = valid_values();
        values.insert(
            "MONKEYBOT_DISCORD_CLIENT_ID".to_owned(),
            "100000000000000001".to_owned(),
        );
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Missing {
                key: "MONKEYBOT_DISCORD_CLIENT_SECRET"
            })
        ));

        values.insert(
            "MONKEYBOT_DISCORD_CLIENT_SECRET".to_owned(),
            "secret".to_owned(),
        );
        values.insert(
            "MONKEYBOT_OAUTH_ENCRYPTION_KEY".to_owned(),
            URL_SAFE_NO_PAD.encode([1_u8; 31]),
        );
        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid {
                key: "MONKEYBOT_OAUTH_ENCRYPTION_KEY",
                ..
            })
        ));
    }

    #[test]
    fn database_is_required_outside_tests() {
        let mut values = valid_values();
        values.insert("MONKEYBOT_ENV".to_owned(), "local".to_owned());

        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Missing {
                key: "MONKEYBOT_DATABASE_URL"
            })
        ));
    }

    #[test]
    fn rejects_pool_minimum_above_maximum() {
        let mut values = valid_values();
        values.insert(
            "MONKEYBOT_DATABASE_URL".to_owned(),
            "postgresql://localhost/monkeybot".to_owned(),
        );
        values.insert(
            "MONKEYBOT_DATABASE_MIN_CONNECTIONS".to_owned(),
            "5".to_owned(),
        );
        values.insert(
            "MONKEYBOT_DATABASE_MAX_CONNECTIONS".to_owned(),
            "4".to_owned(),
        );

        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid {
                key: "MONKEYBOT_DATABASE_MIN_CONNECTIONS",
                ..
            })
        ));
    }

    #[test]
    fn rejects_missing_required_value() {
        let mut values = valid_values();
        values.remove("MONKEYBOT_PUBLIC_ORIGIN");

        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Missing {
                key: "MONKEYBOT_PUBLIC_ORIGIN"
            })
        ));
    }

    #[test]
    fn rejects_non_https_production_origin() {
        let mut values = valid_values();
        values.insert("MONKEYBOT_ENV".to_owned(), "production".to_owned());

        assert!(matches!(
            Config::from_map(&values),
            Err(ConfigError::Invalid {
                key: "MONKEYBOT_PUBLIC_ORIGIN",
                ..
            })
        ));
    }

    #[test]
    fn debug_output_contains_no_environment_values_beyond_config_fields() {
        let mut values = valid_values();
        values.insert(
            "MONKEYBOT_LOG_FILTER".to_owned(),
            "monkeybot_api=debug".to_owned(),
        );
        values.insert(
            "MONKEYBOT_DATABASE_URL".to_owned(),
            "postgresql://runtime:debug-secret@localhost/monkeybot".to_owned(),
        );
        let config = Config::from_map(&values).expect("config should parse");
        let output = format!("{config:?}");

        assert!(output.contains("Config"));
        assert!(!output.contains("PASSWORD"));
        assert!(!output.contains("TOKEN"));
        assert!(!output.contains("debug-secret"));
    }

    #[test]
    fn migration_config_requires_its_own_redacted_url() {
        let config = MigrationConfig::from_lookup(|key| match key {
            "MONKEYBOT_MIGRATION_DATABASE_URL" => {
                Some("postgresql://migrator:migration-secret@localhost/monkeybot".to_owned())
            }
            _ => None,
        })
        .expect("migration config should parse");

        assert_eq!(config.acquire_timeout(), Duration::from_secs(1));
        assert!(!format!("{config:?}").contains("migration-secret"));
    }
}
