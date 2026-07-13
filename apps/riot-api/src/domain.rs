use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Platform {
    Br1,
    Eun1,
    Euw1,
    Jp1,
    Kr,
    La1,
    La2,
    Na1,
    Oc1,
    Ph2,
    Ru,
    Sg2,
    Th2,
    Tr1,
    Tw2,
    Vn2,
}

impl Platform {
    pub fn parse(value: &str) -> Result<Self, PlatformError> {
        match value.trim().to_ascii_uppercase().as_str() {
            "BR" | "BR1" => Ok(Self::Br1),
            "EUNE" | "EUN1" => Ok(Self::Eun1),
            "EUW" | "EUW1" => Ok(Self::Euw1),
            "JP" | "JP1" => Ok(Self::Jp1),
            "KR" => Ok(Self::Kr),
            "LAN" | "LA1" => Ok(Self::La1),
            "LAS" | "LA2" => Ok(Self::La2),
            "NA" | "NA1" => Ok(Self::Na1),
            "OCE" | "OC1" => Ok(Self::Oc1),
            "PH" | "PH2" => Ok(Self::Ph2),
            "RU" => Ok(Self::Ru),
            "SG" | "SG2" => Ok(Self::Sg2),
            "TH" | "TH2" => Ok(Self::Th2),
            "TR" | "TR1" => Ok(Self::Tr1),
            "TW" | "TW2" => Ok(Self::Tw2),
            "VN" | "VN2" => Ok(Self::Vn2),
            _ => Err(PlatformError),
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Br1 => "BR1",
            Self::Eun1 => "EUN1",
            Self::Euw1 => "EUW1",
            Self::Jp1 => "JP1",
            Self::Kr => "KR",
            Self::La1 => "LA1",
            Self::La2 => "LA2",
            Self::Na1 => "NA1",
            Self::Oc1 => "OC1",
            Self::Ph2 => "PH2",
            Self::Ru => "RU",
            Self::Sg2 => "SG2",
            Self::Th2 => "TH2",
            Self::Tr1 => "TR1",
            Self::Tw2 => "TW2",
            Self::Vn2 => "VN2",
        }
    }

    pub fn host(self) -> String {
        format!("{}.api.riotgames.com", self.code().to_ascii_lowercase())
    }

    pub const fn routing(self) -> RoutingRegion {
        match self {
            Self::Br1 | Self::La1 | Self::La2 | Self::Na1 | Self::Oc1 => RoutingRegion::Americas,
            Self::Eun1 | Self::Euw1 | Self::Ru | Self::Tr1 => RoutingRegion::Europe,
            Self::Jp1 | Self::Kr => RoutingRegion::Asia,
            Self::Ph2 | Self::Sg2 | Self::Th2 | Self::Tw2 | Self::Vn2 => RoutingRegion::Sea,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RoutingRegion {
    Americas,
    Asia,
    Europe,
    Sea,
}

impl RoutingRegion {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Americas => "AMERICAS",
            Self::Asia => "ASIA",
            Self::Europe => "EUROPE",
            Self::Sea => "SEA",
        }
    }

    pub fn host(self) -> String {
        format!("{}.api.riotgames.com", self.code().to_ascii_lowercase())
    }
}

#[derive(Debug, Error)]
#[error("unsupported Riot platform")]
pub struct PlatformError;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RiotIdInput {
    pub game_name: String,
    pub tag_line: String,
    pub platform: String,
}

impl RiotIdInput {
    pub fn validate(self) -> Result<ValidatedRiotId, InputError> {
        let game_name = validate_part(self.game_name, 64)?;
        let tag_line = validate_part(self.tag_line, 16)?;
        let platform = Platform::parse(&self.platform).map_err(|_| InputError::Platform)?;
        Ok(ValidatedRiotId {
            game_name,
            tag_line,
            platform,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ValidatedRiotId {
    pub game_name: String,
    pub tag_line: String,
    pub platform: Platform,
}

#[derive(Debug, Error)]
pub enum InputError {
    #[error("Riot ID is invalid")]
    RiotId,
    #[error("Riot platform is invalid")]
    Platform,
    #[error("pagination parameters are invalid")]
    Pagination,
}

fn validate_part(value: String, max_chars: usize) -> Result<String, InputError> {
    let trimmed = value.trim();
    let count = trimmed.chars().count();
    if count == 0 || count > max_chars || trimmed.chars().any(char::is_control) {
        return Err(InputError::RiotId);
    }
    Ok(trimmed.to_owned())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIdentity {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAccount {
    pub account: AccountIdentity,
    pub platform: String,
    pub routing_region: String,
    pub profile_icon_id: Option<i32>,
    pub summoner_level: Option<i64>,
    pub synced_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordsSnapshot {
    pub account: AccountIdentity,
    pub platform: String,
    pub routing_region: String,
    pub profile_icon_id: Option<i32>,
    pub summoner_level: Option<i64>,
    pub ranks: Vec<RankEntry>,
    pub champion_aggregates: Vec<ChampionAggregate>,
    pub recent_matches: Vec<MatchSummary>,
    pub sample: RecordsSampleMetadata,
    pub refreshed_at: String,
    pub cache: CacheMetadata,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordsSampleMetadata {
    pub recent_matches: RecentMatchSampleMetadata,
    pub champion_aggregates: ChampionAggregateSampleMetadata,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentMatchSampleMetadata {
    pub requested: usize,
    pub returned: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionAggregateSampleMetadata {
    pub season_start_unix_seconds: i64,
    pub discovered_matches: usize,
    pub analyzed_matches: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentMatchesPage {
    pub account: AccountIdentity,
    pub platform: String,
    pub routing_region: String,
    pub target_puuid: String,
    pub matches: Vec<MatchSummary>,
    pub page: RecentMatchesPageMetadata,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentMatchesPageMetadata {
    pub offset: usize,
    pub requested: usize,
    pub returned: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankEntry {
    pub queue_type: String,
    pub tier: Option<String>,
    pub rank: Option<String>,
    pub league_points: i32,
    pub wins: i32,
    pub losses: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionAggregate {
    pub champion_name: String,
    pub games: u32,
    pub wins: u32,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub creep_score: i64,
    pub gold_earned: i64,
    pub triple_kills: i64,
    pub quadra_kills: i64,
    pub penta_kills: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    pub match_id: String,
    pub queue_id: i32,
    pub game_version: String,
    pub duration_seconds: i64,
    pub started_at: String,
    pub target_puuid: String,
    pub participants: Vec<MatchParticipant>,
    pub teams: Vec<MatchTeam>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchParticipant {
    pub puuid: String,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub champion_name: String,
    pub champion_level: i32,
    pub team_id: i32,
    pub win: bool,
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
    pub creep_score: i32,
    pub gold_earned: i32,
    pub damage_dealt_to_champions: i32,
    pub damage_taken: i32,
    pub vision_score: i32,
    pub summoner_spell_ids: [i32; 2],
    pub item_ids: [i32; 7],
    pub triple_kills: i32,
    pub quadra_kills: i32,
    pub penta_kills: i32,
    pub team_position: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchTeam {
    pub team_id: i32,
    pub win: bool,
    pub objectives: TeamObjectives,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamObjectives {
    pub baron_kills: i32,
    pub champion_kills: i32,
    pub dragon_kills: i32,
    pub horde_kills: i32,
    pub inhibitor_kills: i32,
    pub rift_herald_kills: i32,
    pub tower_kills: i32,
}

#[derive(Clone, Debug, Serialize)]
pub struct CacheMetadata {
    pub state: CacheState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheState {
    Fresh,
    Stale,
    Refreshed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_platform_to_provider_hosts() {
        assert_eq!(Platform::parse("kr").expect("KR"), Platform::Kr);
        assert_eq!(Platform::Kr.host(), "kr.api.riotgames.com");
        assert_eq!(Platform::Kr.routing(), RoutingRegion::Asia);
        assert_eq!(Platform::Kr.routing().host(), "asia.api.riotgames.com");
    }

    #[test]
    fn rejects_unknown_platforms_and_control_characters() {
        assert!(Platform::parse("moon1").is_err());
        assert!(
            RiotIdInput {
                game_name: "bad\nname".into(),
                tag_line: "KR1".into(),
                platform: "KR".into(),
            }
            .validate()
            .is_err()
        );
    }
}
