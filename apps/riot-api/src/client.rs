use std::time::Duration;

use futures_util::future::BoxFuture;
use reqwest::{Client, StatusCode, Url, header::HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::domain::{Platform, RoutingRegion};

const USER_AGENT: &str = "MonkeyBot-Riot/0.1 (+https://monkeybot.lol)";

pub trait RiotClient: Send + Sync {
    fn account_by_riot_id<'a>(
        &'a self,
        routing: RoutingRegion,
        game_name: &'a str,
        tag_line: &'a str,
    ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>>;

    fn account_by_puuid<'a>(
        &'a self,
        routing: RoutingRegion,
        puuid: &'a str,
    ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>>;

    fn summoner_by_puuid<'a>(
        &'a self,
        platform: Platform,
        puuid: &'a str,
    ) -> BoxFuture<'a, Result<SummonerDto, RiotClientError>>;

    fn league_entries_by_puuid<'a>(
        &'a self,
        platform: Platform,
        puuid: &'a str,
    ) -> BoxFuture<'a, Result<Vec<LeagueEntryDto>, RiotClientError>>;

    fn match_ids<'a>(
        &'a self,
        routing: RoutingRegion,
        puuid: &'a str,
        query: MatchIdsQuery,
    ) -> BoxFuture<'a, Result<Vec<String>, RiotClientError>>;

    fn match_detail<'a>(
        &'a self,
        routing: RoutingRegion,
        match_id: &'a str,
    ) -> BoxFuture<'a, Result<MatchDto, RiotClientError>>;
}

#[derive(Clone)]
pub struct HttpRiotClient {
    http: Client,
    api_key: SecretString,
}

impl HttpRiotClient {
    pub fn new(api_key: SecretString, timeout: Duration) -> Self {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .build()
            .expect("Riot HTTP client initialization should not fail");
        Self { http, api_key }
    }

    async fn get_json<T: DeserializeOwned>(&self, url: Url) -> Result<T, RiotClientError> {
        let response = self
            .http
            .get(url)
            .header("X-Riot-Token", self.api_key.expose_secret())
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let retry_after_seconds = response
                .headers()
                .get("retry-after")
                .and_then(parse_retry_after);
            return Err(classify_status(status, retry_after_seconds));
        }
        response
            .json::<T>()
            .await
            .map_err(|_| RiotClientError::MalformedResponse)
    }
}

impl RiotClient for HttpRiotClient {
    fn account_by_riot_id<'a>(
        &'a self,
        routing: RoutingRegion,
        game_name: &'a str,
        tag_line: &'a str,
    ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>> {
        Box::pin(async move {
            let url = endpoint(
                &routing.host(),
                &[
                    "riot",
                    "account",
                    "v1",
                    "accounts",
                    "by-riot-id",
                    game_name,
                    tag_line,
                ],
            )?;
            self.get_json(url).await
        })
    }

    fn account_by_puuid<'a>(
        &'a self,
        routing: RoutingRegion,
        puuid: &'a str,
    ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>> {
        Box::pin(async move {
            let url = endpoint(
                &routing.host(),
                &["riot", "account", "v1", "accounts", "by-puuid", puuid],
            )?;
            self.get_json(url).await
        })
    }

    fn summoner_by_puuid<'a>(
        &'a self,
        platform: Platform,
        puuid: &'a str,
    ) -> BoxFuture<'a, Result<SummonerDto, RiotClientError>> {
        Box::pin(async move {
            let url = endpoint(
                &platform.host(),
                &["lol", "summoner", "v4", "summoners", "by-puuid", puuid],
            )?;
            self.get_json(url).await
        })
    }

    fn league_entries_by_puuid<'a>(
        &'a self,
        platform: Platform,
        puuid: &'a str,
    ) -> BoxFuture<'a, Result<Vec<LeagueEntryDto>, RiotClientError>> {
        Box::pin(async move {
            let url = endpoint(
                &platform.host(),
                &["lol", "league", "v4", "entries", "by-puuid", puuid],
            )?;
            self.get_json(url).await
        })
    }

    fn match_ids<'a>(
        &'a self,
        routing: RoutingRegion,
        puuid: &'a str,
        query: MatchIdsQuery,
    ) -> BoxFuture<'a, Result<Vec<String>, RiotClientError>> {
        Box::pin(async move {
            let url = match_ids_endpoint(routing, puuid, query)?;
            self.get_json(url).await
        })
    }

    fn match_detail<'a>(
        &'a self,
        routing: RoutingRegion,
        match_id: &'a str,
    ) -> BoxFuture<'a, Result<MatchDto, RiotClientError>> {
        Box::pin(async move {
            let url = endpoint(
                &routing.host(),
                &["lol", "match", "v5", "matches", match_id],
            )?;
            self.get_json(url).await
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchIdsQuery {
    pub start: usize,
    pub count: usize,
    pub start_time: Option<i64>,
    pub match_type: Option<MatchType>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchType {
    Ranked,
}

impl MatchType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ranked => "ranked",
        }
    }
}

fn match_ids_endpoint(
    routing: RoutingRegion,
    puuid: &str,
    query: MatchIdsQuery,
) -> Result<Url, RiotClientError> {
    let mut url = endpoint(
        &routing.host(),
        &["lol", "match", "v5", "matches", "by-puuid", puuid, "ids"],
    )?;
    let mut query_pairs = url.query_pairs_mut();
    query_pairs
        .append_pair("start", &query.start.to_string())
        .append_pair("count", &query.count.to_string());
    if let Some(start_time) = query.start_time {
        query_pairs.append_pair("startTime", &start_time.to_string());
    }
    if let Some(match_type) = query.match_type {
        query_pairs.append_pair("type", match_type.as_str());
    }
    drop(query_pairs);
    Ok(url)
}

fn endpoint(host: &str, segments: &[&str]) -> Result<Url, RiotClientError> {
    let mut url =
        Url::parse(&format!("https://{host}/")).map_err(|_| RiotClientError::InvalidUrl)?;
    url.path_segments_mut()
        .map_err(|_| RiotClientError::InvalidUrl)?
        .extend(segments.iter().copied());
    Ok(url)
}

fn parse_retry_after(value: &HeaderValue) -> Option<u64> {
    value.to_str().ok()?.parse().ok()
}

fn classify_reqwest_error(error: reqwest::Error) -> RiotClientError {
    if error.is_timeout() {
        RiotClientError::Timeout
    } else {
        RiotClientError::Unavailable
    }
}

fn classify_status(status: StatusCode, retry_after_seconds: Option<u64>) -> RiotClientError {
    match status {
        StatusCode::BAD_REQUEST => RiotClientError::InvalidRequest,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => RiotClientError::Unauthorized,
        StatusCode::NOT_FOUND => RiotClientError::NotFound,
        StatusCode::TOO_MANY_REQUESTS => RiotClientError::RateLimited {
            retry_after_seconds,
        },
        status if status.is_server_error() => RiotClientError::Unavailable,
        status => RiotClientError::Rejected(status.as_u16()),
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotAccountDto {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummonerDto {
    pub profile_icon_id: Option<i32>,
    pub summoner_level: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeagueEntryDto {
    pub queue_type: String,
    pub tier: Option<String>,
    pub rank: Option<String>,
    #[serde(default)]
    pub league_points: i32,
    #[serde(default)]
    pub wins: i32,
    #[serde(default)]
    pub losses: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MatchDto {
    pub metadata: MatchMetadataDto,
    pub info: MatchInfoDto,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MatchMetadataDto {
    #[serde(rename = "matchId")]
    pub match_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchInfoDto {
    pub game_creation: i64,
    pub game_duration: i64,
    pub game_version: String,
    pub queue_id: i32,
    pub participants: Vec<MatchParticipantDto>,
    #[serde(default)]
    pub teams: Vec<MatchTeamDto>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchParticipantDto {
    pub puuid: String,
    pub riot_id_game_name: Option<String>,
    pub riot_id_tagline: Option<String>,
    pub champion_name: String,
    pub champ_level: i32,
    pub team_id: i32,
    pub win: bool,
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
    pub total_minions_killed: i32,
    pub neutral_minions_killed: i32,
    pub gold_earned: i32,
    pub total_damage_dealt_to_champions: i32,
    pub total_damage_taken: i32,
    pub vision_score: i32,
    pub summoner1_id: i32,
    pub summoner2_id: i32,
    pub item0: i32,
    pub item1: i32,
    pub item2: i32,
    pub item3: i32,
    pub item4: i32,
    pub item5: i32,
    pub item6: i32,
    #[serde(default)]
    pub triple_kills: i32,
    #[serde(default)]
    pub quadra_kills: i32,
    #[serde(default)]
    pub penta_kills: i32,
    #[serde(default)]
    pub team_position: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchTeamDto {
    pub team_id: i32,
    pub win: bool,
    #[serde(default)]
    pub objectives: MatchObjectivesDto,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchObjectivesDto {
    #[serde(default)]
    pub baron: MatchObjectiveDto,
    #[serde(default)]
    pub champion: MatchObjectiveDto,
    #[serde(default)]
    pub dragon: MatchObjectiveDto,
    #[serde(default)]
    pub horde: MatchObjectiveDto,
    #[serde(default)]
    pub inhibitor: MatchObjectiveDto,
    #[serde(default)]
    pub rift_herald: MatchObjectiveDto,
    #[serde(default)]
    pub tower: MatchObjectiveDto,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MatchObjectiveDto {
    #[serde(default)]
    pub kills: i32,
}

#[derive(Debug, Error)]
pub enum RiotClientError {
    #[error("Riot request is invalid")]
    InvalidRequest,
    #[error("Riot API credentials were rejected")]
    Unauthorized,
    #[error("Riot resource was not found")]
    NotFound,
    #[error("Riot API rate limit was exceeded")]
    RateLimited { retry_after_seconds: Option<u64> },
    #[error("Riot API request timed out")]
    Timeout,
    #[error("Riot API is unavailable")]
    Unavailable,
    #[error("Riot API rejected the request with status {0}")]
    Rejected(u16),
    #[error("Riot API response was malformed")]
    MalformedResponse,
    #[error("Riot API URL could not be constructed")]
    InvalidUrl,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_encodes_user_controlled_segments() {
        let url = endpoint(
            "asia.api.riotgames.com",
            &[
                "riot",
                "account",
                "v1",
                "accounts",
                "by-riot-id",
                "A/B",
                "KR#1",
            ],
        )
        .expect("url");
        assert_eq!(
            url.as_str(),
            "https://asia.api.riotgames.com/riot/account/v1/accounts/by-riot-id/A%2FB/KR%231"
        );
    }

    #[test]
    fn rate_limit_retains_retry_after() {
        assert!(matches!(
            classify_status(StatusCode::TOO_MANY_REQUESTS, Some(3)),
            RiotClientError::RateLimited {
                retry_after_seconds: Some(3)
            }
        ));
    }

    #[test]
    fn provider_rank_and_match_fields_decode_without_losing_page_data() {
        let rank = serde_json::from_value::<LeagueEntryDto>(serde_json::json!({
            "queueType": "RANKED_SOLO_5x5",
            "tier": "PLATINUM",
            "rank": "III",
            "leaguePoints": 23,
            "wins": 22,
            "losses": 29
        }))
        .expect("league entry");
        assert_eq!(rank.queue_type, "RANKED_SOLO_5x5");
        assert_eq!(rank.tier.as_deref(), Some("PLATINUM"));
        assert_eq!(rank.rank.as_deref(), Some("III"));
        assert_eq!((rank.league_points, rank.wins, rank.losses), (23, 22, 29));

        let game = serde_json::from_value::<MatchDto>(serde_json::json!({
            "metadata": { "matchId": "KR_1" },
            "info": {
                "gameCreation": 1720000000000_i64,
                "gameDuration": 1800,
                "gameVersion": "16.13.1",
                "queueId": 420,
                "participants": [{
                    "puuid": "target-puuid",
                    "riotIdGameName": "Longcat",
                    "riotIdTagline": "KR1",
                    "championName": "LeeSin",
                    "champLevel": 18,
                    "teamId": 100,
                    "win": true,
                    "kills": 10,
                    "deaths": 2,
                    "assists": 8,
                    "totalMinionsKilled": 200,
                    "neutralMinionsKilled": 20,
                    "goldEarned": 14000,
                    "totalDamageDealtToChampions": 30000,
                    "totalDamageTaken": 20000,
                    "visionScore": 40,
                    "summoner1Id": 4,
                    "summoner2Id": 11,
                    "item0": 1001,
                    "item1": 1002,
                    "item2": 1003,
                    "item3": 1004,
                    "item4": 1005,
                    "item5": 1006,
                    "item6": 3364,
                    "tripleKills": 1,
                    "quadraKills": 1,
                    "pentaKills": 1,
                    "teamPosition": "JUNGLE"
                }],
                "teams": [{
                    "teamId": 100,
                    "win": true,
                    "objectives": {
                        "baron": {"first": true, "kills": 1},
                        "champion": {"first": true, "kills": 40},
                        "dragon": {"first": true, "kills": 3},
                        "inhibitor": {"first": true, "kills": 2},
                        "riftHerald": {"first": true, "kills": 1},
                        "tower": {"first": true, "kills": 11}
                    }
                }]
            }
        }))
        .expect("match detail");
        assert_eq!(game.info.participants[0].triple_kills, 1);
        assert_eq!(game.info.participants[0].penta_kills, 1);
        assert_eq!(game.info.teams[0].objectives.tower.kills, 11);
        assert_eq!(game.info.teams[0].objectives.horde.kills, 0);
    }

    #[test]
    fn season_match_id_query_uses_start_time_and_ranked_filter() {
        let url = match_ids_endpoint(
            RoutingRegion::Asia,
            "target-puuid",
            MatchIdsQuery {
                start: 100,
                count: 100,
                start_time: Some(1_735_689_600),
                match_type: Some(MatchType::Ranked),
            },
        )
        .expect("match IDs URL");
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(query.get("start").map(|value| value.as_ref()), Some("100"));
        assert_eq!(query.get("count").map(|value| value.as_ref()), Some("100"));
        assert_eq!(
            query.get("startTime").map(|value| value.as_ref()),
            Some("1735689600")
        );
        assert_eq!(
            query.get("type").map(|value| value.as_ref()),
            Some("ranked")
        );
    }
}
