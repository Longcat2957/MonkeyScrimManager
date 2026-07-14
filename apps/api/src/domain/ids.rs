use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct UserId(String);

impl UserId {
    pub fn parse(value: impl Into<String>) -> Result<Self, UserIdError> {
        let value = value.into();
        validate_user_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for UserId {
    type Err = UserIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for UserId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

macro_rules! snowflake_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, sqlx::Type)]
        #[serde(transparent)]
        #[sqlx(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, SnowflakeError> {
                let value = value.into();
                validate_snowflake(&value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = SnowflakeError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(D::Error::custom)
            }
        }
    };
}

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            Serialize,
            Deserialize,
            sqlx::Type,
        )]
        #[serde(transparent)]
        #[sqlx(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

snowflake_id!(GuildId);
snowflake_id!(DiscordUserId);
snowflake_id!(DiscordRoleId);
snowflake_id!(DiscordChannelId);
snowflake_id!(DiscordMessageId);
snowflake_id!(DiscordInteractionId);

impl From<DiscordUserId> for UserId {
    fn from(value: DiscordUserId) -> Self {
        Self(value.0)
    }
}

impl From<&DiscordUserId> for UserId {
    fn from(value: &DiscordUserId) -> Self {
        Self(value.0.clone())
    }
}

impl TryFrom<UserId> for DiscordUserId {
    type Error = SnowflakeError;

    fn try_from(value: UserId) -> Result<Self, Self::Error> {
        Self::parse(value.0)
    }
}

uuid_id!(EntityId);
uuid_id!(SeasonId);
uuid_id!(RecruitmentId);
uuid_id!(SeriesId);
uuid_id!(AuctionRecruitmentId);
uuid_id!(AuctionTournamentId);
uuid_id!(AuctionMatchId);
uuid_id!(GameId);
uuid_id!(IdempotencyRecordId);
uuid_id!(OutboxEventId);
uuid_id!(NotificationId);

fn validate_snowflake(value: &str) -> Result<(), SnowflakeError> {
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(SnowflakeError);
    }
    value.parse::<u64>().map_err(|_| SnowflakeError)?;
    Ok(())
}

fn validate_user_id(value: &str) -> Result<(), UserIdError> {
    if validate_snowflake(value).is_ok() {
        return Ok(());
    }

    let Some(dummy_uuid) = value.strip_prefix("dmy_") else {
        return Err(UserIdError);
    };
    let parsed = Uuid::parse_str(dummy_uuid).map_err(|_| UserIdError)?;
    let has_rfc_4122_variant = matches!(
        dummy_uuid.as_bytes().get(19),
        Some(b'8' | b'9' | b'a' | b'b')
    );
    if parsed.get_version_num() != 7
        || !has_rfc_4122_variant
        || parsed.hyphenated().to_string() != dummy_uuid
    {
        return Err(UserIdError);
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("user ID must be a Discord snowflake or canonical dmy_<UUIDv7>")]
pub struct UserIdError;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("Discord snowflake must be a non-zero unsigned decimal string")]
pub struct SnowflakeError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snowflakes_are_strict_decimal_strings() {
        assert!(GuildId::parse("123456789012345678").is_ok());
        for invalid in ["", "0", "01", "-1", "123x", "18446744073709551616"] {
            assert!(GuildId::parse(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn snowflake_deserialization_preserves_the_validation_boundary() {
        let guild_id = serde_json::from_str::<GuildId>("\"123456789012345678\"")
            .expect("valid guild ID should deserialize");
        assert_eq!(guild_id.as_str(), "123456789012345678");

        for invalid in [
            "\"\"",
            "\"0\"",
            "\"01\"",
            "\"not-a-snowflake\"",
            "\"18446744073709551616\"",
        ] {
            assert!(
                serde_json::from_str::<GuildId>(invalid).is_err(),
                "deserialized {invalid}"
            );
            assert!(
                serde_json::from_str::<DiscordUserId>(invalid).is_err(),
                "deserialized {invalid}"
            );
            assert!(
                serde_json::from_str::<DiscordInteractionId>(invalid).is_err(),
                "deserialized interaction ID {invalid}"
            );
        }
    }

    #[test]
    fn new_domain_ids_use_uuid_v7() {
        assert_eq!(EntityId::new().as_uuid().get_version_num(), 7);
    }

    #[test]
    fn user_ids_accept_discord_and_internal_dummy_identities() {
        let discord = DiscordUserId::parse("123456789012345678").expect("Discord user ID");
        assert_eq!(UserId::from(&discord).as_str(), discord.as_str());
        assert!(UserId::parse("dmy_01900000-0000-7000-8000-000000000001").is_ok());

        for invalid in [
            "",
            "0",
            "01",
            "user",
            " user",
            "user ",
            "user\n",
            "dmy_01900000-0000-6000-8000-000000000001",
            "dmy_01900000-0000-7000-7000-000000000001",
            "dmy_01900000-0000-7000-8000-000000000001 ",
            "dmy_01900000-0000-7000-8000-00000000000A",
        ] {
            assert!(UserId::parse(invalid).is_err(), "accepted {invalid:?}");
        }
        assert!(UserId::parse("x".repeat(129)).is_err());
    }
}
