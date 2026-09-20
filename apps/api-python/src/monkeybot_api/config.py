import base64
import ipaddress
import re
from pathlib import Path
from typing import Literal, Self
from urllib.parse import urlsplit

from pydantic import Field, SecretStr, model_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


def is_snowflake(value: str) -> bool:
    return bool(re.fullmatch(r"[1-9][0-9]{0,19}", value)) and int(value) <= 2**64 - 1


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="MONKEYBOT_", extra="ignore", hide_input_in_errors=True
    )

    env: Literal["local", "development", "production", "test"]
    bind_addr: str = "0.0.0.0:8080"
    public_origin: str
    database_url: SecretStr | None = None
    valkey_url: SecretStr | None = None
    discord_application_id: str | None = None
    discord_client_secret: SecretStr | None = None
    oauth_encryption_key: SecretStr | None = None
    bot_service_token: SecretStr | None = None
    service_admin_ids: str = ""
    session_ttl_seconds: int = Field(default=604_800, ge=300, le=2_592_000)
    request_timeout_ms: int = Field(default=10_000, ge=1, le=120_000)
    shutdown_timeout_ms: int = Field(default=20_000, ge=1, le=120_000)
    max_body_bytes: int = Field(default=16_777_216, ge=1, le=16_777_216)
    database_max_connections: int = Field(default=10, ge=1, le=100)
    database_acquire_timeout_ms: int = Field(default=1_000, ge=1, le=120_000)
    database_probe_timeout_ms: int = Field(default=1_000, ge=1, le=120_000)
    database_statement_timeout_ms: int = Field(default=3_000, ge=1, le=120_000)
    database_lock_timeout_ms: int = Field(default=1_000, ge=1, le=120_000)
    database_idle_transaction_timeout_ms: int = Field(default=10_000, ge=1, le=120_000)
    database_max_lifetime_ms: int = Field(default=1_800_000, ge=1, le=86_400_000)
    migration_manifest: Path = Path(__file__).with_name("sqlx-manifest.json")

    @model_validator(mode="after")
    def validate_contract(self) -> Self:
        if self.bot_service_token is not None:
            token = self.bot_service_token.get_secret_value()
            if not token.strip():
                self.bot_service_token = None
            elif len(token) < 32:
                raise ValueError("MONKEYBOT_BOT_SERVICE_TOKEN must contain at least 32 characters")
        origin = urlsplit(self.public_origin)
        if (
            origin.scheme not in {"http", "https"}
            or not origin.hostname
            or origin.username is not None
            or origin.password is not None
            or origin.path not in {"", "/"}
            or origin.query
            or origin.fragment
            or (self.env == "production" and origin.scheme != "https")
        ):
            raise ValueError("MONKEYBOT_PUBLIC_ORIGIN must be an origin; production requires HTTPS")
        self.public_origin = self.public_origin.rstrip("/")
        self.bind_host_port()
        if self.env != "test" and self.database_url is None:
            raise ValueError("MONKEYBOT_DATABASE_URL is required")
        if self.database_url is not None:
            database = urlsplit(self.database_url.get_secret_value())
            if database.scheme not in {"postgresql", "postgres"} or not database.hostname:
                raise ValueError("MONKEYBOT_DATABASE_URL must be a PostgreSQL URL")
        if self.valkey_url is not None:
            valkey = urlsplit(self.valkey_url.get_secret_value())
            if valkey.scheme != "redis" or not valkey.hostname or valkey.query or valkey.fragment:
                raise ValueError("MONKEYBOT_VALKEY_URL must be a redis URL without query/fragment")
        auth_fields = (
            self.discord_application_id,
            self.discord_client_secret,
            self.oauth_encryption_key,
        )
        if self.env != "test" or any(value is not None for value in auth_fields):
            if not all(value is not None for value in auth_fields):
                raise ValueError("Discord application ID, client secret and OAuth key are required")
            assert self.discord_application_id is not None
            assert self.oauth_encryption_key is not None
            assert self.discord_client_secret is not None
            if not is_snowflake(self.discord_application_id):
                raise ValueError("MONKEYBOT_DISCORD_APPLICATION_ID must be a canonical snowflake")
            if not self.discord_client_secret.get_secret_value().strip():
                raise ValueError("MONKEYBOT_DISCORD_CLIENT_SECRET must not be empty")
            key = self.oauth_encryption_key.get_secret_value()
            if not re.fullmatch(r"[A-Za-z0-9_-]{43}", key):
                raise ValueError(
                    "MONKEYBOT_OAUTH_ENCRYPTION_KEY must encode 32 bytes, without padding"
                )
            decoded = base64.urlsafe_b64decode(key + "=")
            if base64.urlsafe_b64encode(decoded).decode().rstrip("=") != key:
                raise ValueError("MONKEYBOT_OAUTH_ENCRYPTION_KEY must use canonical base64url")
        if any(not is_snowflake(value) for value in self.admin_ids):
            raise ValueError("MONKEYBOT_SERVICE_ADMIN_IDS must contain comma-separated snowflakes")
        return self

    @property
    def admin_ids(self) -> frozenset[str]:
        return frozenset(
            value.strip() for value in self.service_admin_ids.split(",") if value.strip()
        )

    @property
    def auth_configured(self) -> bool:
        return self.discord_application_id is not None

    def bind_host_port(self) -> tuple[str, int]:
        try:
            host, port_value = self.bind_addr.rsplit(":", 1)
            host = host.removeprefix("[").removesuffix("]")
            ipaddress.ip_address(host)
            port = int(port_value)
            if not 1 <= port <= 65535:
                raise ValueError
        except ValueError:
            raise ValueError("MONKEYBOT_BIND_ADDR must be an IP address and nonzero port") from None
        return host, port
