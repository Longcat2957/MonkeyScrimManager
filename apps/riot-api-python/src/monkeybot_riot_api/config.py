"""Runtime settings matching the Rust Riot service environment contract."""

from ipaddress import ip_address

from pydantic import Field, SecretStr, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="MONKEYBOT_RIOT_", extra="ignore")

    bind_addr: str = "127.0.0.1:30040"
    api_key: SecretStr
    internal_token: SecretStr
    request_timeout_ms: int = Field(default=5000, ge=500, le=30000)
    match_count: int = Field(default=10, ge=1, le=20)
    season_start_unix_seconds: int = Field(gt=0)

    @field_validator("bind_addr")
    @classmethod
    def socket_address(cls, value: str) -> str:
        host, separator, port = value.rpartition(":")
        if not separator or not host or not port.isascii() or not port.isdecimal():
            raise ValueError("bind address must be an IP address and port")
        if not 0 <= int(port) <= 65535:
            raise ValueError("bind port is invalid")
        if host.startswith("[") and host.endswith("]"):
            host = host[1:-1]
        elif ":" in host:
            raise ValueError("IPv6 bind address must be bracketed")
        ip_address(host)
        return value

    @field_validator("api_key", "internal_token")
    @classmethod
    def nonempty_secret(cls, value: SecretStr) -> SecretStr:
        if not value.get_secret_value().strip():
            raise ValueError("secret must be nonempty")
        return value

    @field_validator("internal_token")
    @classmethod
    def long_internal_token(cls, value: SecretStr) -> SecretStr:
        if len(value.get_secret_value()) < 32:
            raise ValueError("internal token must contain at least 32 characters")
        return value
