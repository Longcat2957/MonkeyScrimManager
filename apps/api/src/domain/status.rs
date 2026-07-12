use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

macro_rules! text_status {
    (
        pub enum $name:ident {
            $($variant:ident => $wire:literal),+ $(,)?
        }
    ) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Eq,
            Hash,
            PartialEq,
            Serialize,
            Deserialize,
            sqlx::Type,
        )]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        #[sqlx(type_name = "text", rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = StatusParseError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($wire => Ok(Self::$variant)),+,
                    _ => Err(StatusParseError::new(stringify!($name))),
                }
            }
        }
    };
}

text_status! {
    pub enum GuildStatus {
        Installed => "INSTALLED",
        SetupRequired => "SETUP_REQUIRED",
        Active => "ACTIVE",
        Disabled => "DISABLED",
        Left => "LEFT",
    }
}

text_status! {
    pub enum GuildMemberStatus {
        Active => "ACTIVE",
        Inactive => "INACTIVE",
    }
}

text_status! {
    pub enum SeasonStatus {
        Active => "ACTIVE",
        Closed => "CLOSED",
    }
}

text_status! {
    pub enum RecruitmentStatus {
        Open => "OPEN",
        Closed => "CLOSED",
        Converted => "CONVERTED",
        Cancelled => "CANCELLED",
    }
}

text_status! {
    pub enum ParticipantStatus {
        Joined => "JOINED",
        Withdrawn => "WITHDRAWN",
    }
}

text_status! {
    pub enum SeriesStatus {
        InProgress => "IN_PROGRESS",
        Completed => "COMPLETED",
        Cancelled => "CANCELLED",
    }
}

text_status! {
    pub enum AuctionTournamentStatus {
        Bidding => "BIDDING",
        BracketSetup => "BRACKET_SETUP",
        InGame => "IN_GAME",
        Completed => "COMPLETED",
        Cancelled => "CANCELLED",
    }
}

text_status! {
    pub enum AuctionCandidateStatus {
        Queued => "QUEUED",
        Active => "ACTIVE",
        Unsold => "UNSOLD",
        Assigned => "ASSIGNED",
    }
}

text_status! {
    pub enum AuctionLotStatus {
        Active => "ACTIVE",
        Awarded => "AWARDED",
        Unsold => "UNSOLD",
        Assigned => "ASSIGNED",
        Reverted => "REVERTED",
    }
}

text_status! {
    pub enum AuctionMatchStatus {
        LineupPending => "LINEUP_PENDING",
        Ready => "READY",
        InProgress => "IN_PROGRESS",
        Completed => "COMPLETED",
        Cancelled => "CANCELLED",
    }
}

text_status! {
    pub enum GameInputStatus {
        Draft => "DRAFT",
        Submitted => "SUBMITTED",
        Superseded => "SUPERSEDED",
    }
}

text_status! {
    pub enum GameStatus {
        Confirmed => "CONFIRMED",
        Voided => "VOIDED",
    }
}

text_status! {
    pub enum GameMmrStatus {
        Applied => "APPLIED",
        LegacyNotApplied => "LEGACY_NOT_APPLIED",
        Reversed => "REVERSED",
    }
}

text_status! {
    pub enum RevertCheckpointStatus {
        Active => "ACTIVE",
        Reverted => "REVERTED",
        Expired => "EXPIRED",
    }
}

text_status! {
    pub enum IdempotencyState {
        InProgress => "IN_PROGRESS",
        Completed => "COMPLETED",
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("unsupported canonical value for {status_type}")]
pub struct StatusParseError {
    status_type: &'static str,
}

impl StatusParseError {
    const fn new(status_type: &'static str) -> Self {
        Self { status_type }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::{Serialize, de::DeserializeOwned};
    use sqlx::{Decode, Encode, Postgres, Type, postgres::PgArgumentBuffer};

    use super::*;

    fn assert_roundtrip<T>(values: &'static [T])
    where
        T: Copy
            + Debug
            + Eq
            + FromStr<Err = StatusParseError>
            + Serialize
            + DeserializeOwned
            + Type<Postgres>
            + for<'query> Encode<'query, Postgres>
            + for<'row> Decode<'row, Postgres>
            + fmt::Display,
        T: StatusValue,
    {
        for value in values {
            let wire = value.as_str();

            assert_eq!(value.to_string(), wire);
            assert_eq!(wire.parse::<T>(), Ok(*value));
            assert_eq!(
                serde_json::to_string(value).expect("serialize status"),
                format!("\"{wire}\"")
            );
            assert_eq!(
                serde_json::from_str::<T>(&format!("\"{wire}\""))
                    .expect("deserialize canonical status"),
                *value
            );

            let mut buffer = PgArgumentBuffer::default();
            let encoded = value
                .encode_by_ref(&mut buffer)
                .expect("encode PostgreSQL text status");
            assert!(!encoded.is_null());
            assert_eq!(buffer.as_slice(), wire.as_bytes());
        }

        assert!("invalid".parse::<T>().is_err());
        assert!(serde_json::from_str::<T>("\"invalid\"").is_err());
    }

    trait StatusValue {
        fn as_str(&self) -> &'static str;
    }

    macro_rules! impl_status_value {
        ($($status:ty),+ $(,)?) => {
            $(
                impl StatusValue for $status {
                    fn as_str(&self) -> &'static str {
                        (*self).as_str()
                    }
                }
            )+
        };
    }

    impl_status_value! {
        GuildStatus,
        GuildMemberStatus,
        SeasonStatus,
        RecruitmentStatus,
        ParticipantStatus,
        SeriesStatus,
        AuctionTournamentStatus,
        AuctionCandidateStatus,
        AuctionLotStatus,
        AuctionMatchStatus,
        GameInputStatus,
        GameStatus,
        GameMmrStatus,
        RevertCheckpointStatus,
        IdempotencyState,
    }

    #[test]
    fn status_wire_values_roundtrip_across_text_boundaries() {
        assert_roundtrip(GuildStatus::ALL);
        assert_roundtrip(GuildMemberStatus::ALL);
        assert_roundtrip(SeasonStatus::ALL);
        assert_roundtrip(RecruitmentStatus::ALL);
        assert_roundtrip(ParticipantStatus::ALL);
        assert_roundtrip(SeriesStatus::ALL);
        assert_roundtrip(AuctionTournamentStatus::ALL);
        assert_roundtrip(AuctionCandidateStatus::ALL);
        assert_roundtrip(AuctionLotStatus::ALL);
        assert_roundtrip(AuctionMatchStatus::ALL);
        assert_roundtrip(GameInputStatus::ALL);
        assert_roundtrip(GameStatus::ALL);
        assert_roundtrip(GameMmrStatus::ALL);
        assert_roundtrip(RevertCheckpointStatus::ALL);
        assert_roundtrip(IdempotencyState::ALL);
    }
}
