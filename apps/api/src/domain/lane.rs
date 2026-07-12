use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "UPPERCASE")]
#[sqlx(type_name = "text", rename_all = "UPPERCASE")]
pub enum Lane {
    Top,
    Jungle,
    Mid,
    Bottom,
    Support,
}

impl Lane {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "TOP",
            Self::Jungle => "JUNGLE",
            Self::Mid => "MID",
            Self::Bottom => "BOTTOM",
            Self::Support => "SUPPORT",
        }
    }
}

impl fmt::Display for Lane {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Lane {
    type Err = LaneParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "TOP" => Ok(Self::Top),
            "JUNGLE" => Ok(Self::Jungle),
            "MID" => Ok(Self::Mid),
            "BOTTOM" => Ok(Self::Bottom),
            "SUPPORT" => Ok(Self::Support),
            _ => Err(LaneParseError),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("unsupported canonical lane")]
pub struct LaneParseError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_wire_values_are_canonical() {
        for lane in [
            Lane::Top,
            Lane::Jungle,
            Lane::Mid,
            Lane::Bottom,
            Lane::Support,
        ] {
            assert_eq!(lane.as_str().parse::<Lane>(), Ok(lane));
        }
        assert!("FLEX".parse::<Lane>().is_err());
    }
}
