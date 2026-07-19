use std::{collections::HashSet, fmt, str::FromStr, sync::Arc, time::Duration};

use futures_util::future::BoxFuture;
use reqwest::{Client, StatusCode, Url, header::RETRY_AFTER};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const USER_AGENT: &str = "MonkeyBot/0.1 (+https://monkeybot.lol)";
const MAX_GAME_NAME_CHARS: usize = 64;
const MAX_TAG_LINE_CHARS: usize = 16;
const MAX_PUUID_BYTES: usize = 256;
const MAX_RECORD_MATCHES: usize = 20;
const MAX_MATCH_PARTICIPANTS: usize = 20;
const MAX_SEASON_AGGREGATE_MATCHES: usize = 2_000;
const MAX_RECENT_MATCH_OFFSET: usize = 10_000;

#[derive(Clone)]
pub struct RiotService {
    client: Arc<dyn RiotClient>,
}

impl RiotService {
    pub fn new(client: Arc<dyn RiotClient>) -> Self {
        Self { client }
    }

    pub fn from_internal_service(
        base_url: Url,
        service_token: SecretString,
    ) -> Result<Self, RiotClientError> {
        Ok(Self::new(Arc::new(ReqwestRiotClient::new(
            base_url,
            service_token,
        )?)))
    }

    pub async fn resolve_account(
        &self,
        game_name: &str,
        tag_line: &str,
        platform: RiotPlatform,
    ) -> Result<ResolvedRiotAccount, RiotClientError> {
        let riot_id = RiotId::new(game_name, tag_line)?;
        let response = self
            .client
            .resolve_account(&ResolveAccountRequest {
                platform,
                lookup: AccountLookup::RiotId {
                    game_name: riot_id.game_name,
                    tag_line: riot_id.tag_line,
                },
            })
            .await?;
        response.validate(platform, None)?;
        Ok(response)
    }

    pub async fn resolve_account_by_puuid(
        &self,
        puuid: &str,
        platform: RiotPlatform,
    ) -> Result<ResolvedRiotAccount, RiotClientError> {
        validate_puuid(puuid)?;
        let response = self
            .client
            .resolve_account(&ResolveAccountRequest {
                platform,
                lookup: AccountLookup::Puuid {
                    puuid: puuid.to_owned(),
                },
            })
            .await?;
        response.validate(platform, Some(puuid))?;
        Ok(response)
    }

    pub async fn records(
        &self,
        game_name: &str,
        tag_line: &str,
        platform: RiotPlatform,
        refresh: bool,
    ) -> Result<RiotRecords, RiotClientError> {
        let riot_id = RiotId::new(game_name, tag_line)?;
        let request = RecordsRequest {
            game_name: riot_id.game_name,
            tag_line: riot_id.tag_line,
            platform,
        };
        let response = if refresh {
            self.client.refresh_records(&request).await?
        } else {
            self.client.records(&request).await?
        };
        response.validate(platform)?;
        Ok(response)
    }

    pub async fn recent_matches_page(
        &self,
        game_name: &str,
        tag_line: &str,
        platform: RiotPlatform,
        offset: usize,
        count: usize,
    ) -> Result<RiotRecentMatchesPage, RiotClientError> {
        if !(1..=MAX_RECORD_MATCHES).contains(&count) || offset > MAX_RECENT_MATCH_OFFSET {
            return Err(RiotClientError::InvalidInput);
        }
        let riot_id = RiotId::new(game_name, tag_line)?;
        let request = RecordsRequest {
            game_name: riot_id.game_name,
            tag_line: riot_id.tag_line,
            platform,
        };
        let response = self
            .client
            .recent_matches_page(&request, offset, count)
            .await?;
        response.validate(platform, offset, count)?;
        Ok(response)
    }
}

impl fmt::Debug for RiotService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RiotService(..)")
    }
}

pub trait RiotClient: Send + Sync {
    fn resolve_account<'a>(
        &'a self,
        request: &'a ResolveAccountRequest,
    ) -> BoxFuture<'a, Result<ResolvedRiotAccount, RiotClientError>>;

    fn records<'a>(
        &'a self,
        request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>>;

    fn refresh_records<'a>(
        &'a self,
        request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>>;

    fn recent_matches_page<'a>(
        &'a self,
        request: &'a RecordsRequest,
        offset: usize,
        count: usize,
    ) -> BoxFuture<'a, Result<RiotRecentMatchesPage, RiotClientError>>;
}

#[derive(Clone)]
pub struct ReqwestRiotClient {
    http: Client,
    base_url: Url,
    service_token: SecretString,
}

impl ReqwestRiotClient {
    pub fn new(base_url: Url, service_token: SecretString) -> Result<Self, RiotClientError> {
        if !is_internal_service_base_url(&base_url)
            || service_token.expose_secret().chars().count() < 32
        {
            return Err(RiotClientError::InvalidConfiguration);
        }
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(RiotClientError::Request)?;
        Ok(Self {
            http,
            base_url,
            service_token,
        })
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &RecordsRequest,
    ) -> Result<T, RiotClientError> {
        let mut url = self.url(path)?;
        url.query_pairs_mut()
            .append_pair("gameName", &query.game_name)
            .append_pair("tagLine", &query.tag_line)
            .append_pair("platform", query.platform.as_str());
        let response = self
            .http
            .get(url)
            .bearer_auth(self.service_token.expose_secret())
            .send()
            .await
            .map_err(map_request_error)?;
        decode_response(response).await
    }

    async fn get_recent_matches_page(
        &self,
        query: &RecordsRequest,
        offset: usize,
        count: usize,
    ) -> Result<RiotRecentMatchesPage, RiotClientError> {
        let mut url = self.url("records/matches")?;
        url.query_pairs_mut()
            .append_pair("gameName", &query.game_name)
            .append_pair("tagLine", &query.tag_line)
            .append_pair("platform", query.platform.as_str())
            .append_pair("start", &offset.to_string())
            .append_pair("count", &count.to_string());
        let response = self
            .http
            .get(url)
            .bearer_auth(self.service_token.expose_secret())
            .send()
            .await
            .map_err(map_request_error)?;
        decode_response(response).await
    }

    async fn post_json<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, RiotClientError> {
        let response = self
            .http
            .post(self.url(path)?)
            .bearer_auth(self.service_token.expose_secret())
            .json(body)
            .send()
            .await
            .map_err(map_request_error)?;
        decode_response(response).await
    }

    fn url(&self, path: &str) -> Result<Url, RiotClientError> {
        self.base_url
            .join(path)
            .map_err(|_| RiotClientError::InvalidConfiguration)
    }
}

impl fmt::Debug for ReqwestRiotClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReqwestRiotClient")
            .field("base_url", &self.base_url)
            .field("service_token", &"[REDACTED]")
            .finish()
    }
}

impl RiotClient for ReqwestRiotClient {
    fn resolve_account<'a>(
        &'a self,
        request: &'a ResolveAccountRequest,
    ) -> BoxFuture<'a, Result<ResolvedRiotAccount, RiotClientError>> {
        Box::pin(async move { self.post_json("accounts/resolve", request).await })
    }

    fn records<'a>(
        &'a self,
        request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>> {
        Box::pin(async move { self.get_json("records", request).await })
    }

    fn refresh_records<'a>(
        &'a self,
        request: &'a RecordsRequest,
    ) -> BoxFuture<'a, Result<RiotRecords, RiotClientError>> {
        Box::pin(async move { self.post_json("records/refresh", request).await })
    }

    fn recent_matches_page<'a>(
        &'a self,
        request: &'a RecordsRequest,
        offset: usize,
        count: usize,
    ) -> BoxFuture<'a, Result<RiotRecentMatchesPage, RiotClientError>> {
        Box::pin(async move { self.get_recent_matches_page(request, offset, count).await })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RiotPlatform {
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

impl RiotPlatform {
    pub const fn as_str(self) -> &'static str {
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

    pub const fn routing_region(self) -> RiotRoutingRegion {
        match self {
            Self::Br1 | Self::La1 | Self::La2 | Self::Na1 | Self::Oc1 => {
                RiotRoutingRegion::Americas
            }
            Self::Eun1 | Self::Euw1 | Self::Ru | Self::Tr1 => RiotRoutingRegion::Europe,
            Self::Jp1 | Self::Kr => RiotRoutingRegion::Asia,
            Self::Ph2 | Self::Sg2 | Self::Th2 | Self::Tw2 | Self::Vn2 => RiotRoutingRegion::Sea,
        }
    }
}

impl FromStr for RiotPlatform {
    type Err = RiotClientError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "br" | "br1" => Ok(Self::Br1),
            "eune" | "eun1" => Ok(Self::Eun1),
            "euw" | "euw1" => Ok(Self::Euw1),
            "jp" | "jp1" => Ok(Self::Jp1),
            "kr" => Ok(Self::Kr),
            "lan" | "la1" => Ok(Self::La1),
            "las" | "la2" => Ok(Self::La2),
            "na" | "na1" => Ok(Self::Na1),
            "oce" | "oc1" => Ok(Self::Oc1),
            "ph" | "ph2" => Ok(Self::Ph2),
            "ru" => Ok(Self::Ru),
            "sg" | "sg2" => Ok(Self::Sg2),
            "th" | "th2" => Ok(Self::Th2),
            "tr" | "tr1" => Ok(Self::Tr1),
            "tw" | "tw2" => Ok(Self::Tw2),
            "vn" | "vn2" => Ok(Self::Vn2),
            _ => Err(RiotClientError::UnsupportedPlatform),
        }
    }
}

impl fmt::Display for RiotPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for RiotPlatform {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RiotPlatform {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RiotRoutingRegion {
    Americas,
    Asia,
    Europe,
    Sea,
}

impl RiotRoutingRegion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Americas => "AMERICAS",
            Self::Asia => "ASIA",
            Self::Europe => "EUROPE",
            Self::Sea => "SEA",
        }
    }
}

impl fmt::Display for RiotRoutingRegion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiotId {
    game_name: String,
    tag_line: String,
}

impl RiotId {
    pub fn new(game_name: &str, tag_line: &str) -> Result<Self, RiotClientError> {
        Ok(Self {
            game_name: validate_riot_id_component(game_name, MAX_GAME_NAME_CHARS)?,
            tag_line: validate_riot_id_component(tag_line, MAX_TAG_LINE_CHARS)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveAccountRequest {
    pub platform: RiotPlatform,
    pub lookup: AccountLookup,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AccountLookup {
    RiotId { game_name: String, tag_line: String },
    Puuid { puuid: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordsRequest {
    pub game_name: String,
    pub tag_line: String,
    pub platform: RiotPlatform,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotAccount {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
}

impl RiotAccount {
    fn validate(&self) -> Result<(), RiotClientError> {
        validate_puuid(&self.puuid)?;
        validate_riot_id_component(&self.game_name, MAX_GAME_NAME_CHARS)?;
        validate_riot_id_component(&self.tag_line, MAX_TAG_LINE_CHARS)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRiotAccount {
    pub account: RiotAccount,
    pub platform: RiotPlatform,
    pub routing_region: RiotRoutingRegion,
    pub profile_icon_id: Option<i32>,
    pub summoner_level: Option<i64>,
    pub synced_at: String,
}

impl ResolvedRiotAccount {
    fn validate(
        &self,
        requested_platform: RiotPlatform,
        requested_puuid: Option<&str>,
    ) -> Result<(), RiotClientError> {
        self.account.validate()?;
        if self.platform != requested_platform
            || self.routing_region != requested_platform.routing_region()
            || self.profile_icon_id.is_some_and(|value| value < 0)
            || self.summoner_level.is_some_and(|value| value < 0)
            || self.synced_at.is_empty()
            || requested_puuid.is_some_and(|puuid| puuid != self.account.puuid)
        {
            return Err(RiotClientError::MalformedResponse);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotRecords {
    pub account: RiotAccount,
    pub platform: RiotPlatform,
    pub routing_region: RiotRoutingRegion,
    pub profile_icon_id: Option<i32>,
    pub summoner_level: Option<i64>,
    pub ranks: Vec<RiotRank>,
    pub champion_aggregates: Vec<RiotChampionAggregate>,
    pub recent_matches: Vec<RiotRecentMatch>,
    pub sample: RiotRecordsSample,
    pub refreshed_at: String,
    pub cache: RiotCacheMetadata,
}

impl RiotRecords {
    pub(crate) fn validate(&self, requested_platform: RiotPlatform) -> Result<(), RiotClientError> {
        self.account.validate()?;
        if self.platform != requested_platform
            || self.routing_region != requested_platform.routing_region()
            || self.profile_icon_id.is_some_and(|value| value < 0)
            || self.summoner_level.is_some_and(|value| value < 0)
            || self.refreshed_at.is_empty()
            || self.recent_matches.len() > MAX_RECORD_MATCHES
        {
            return Err(RiotClientError::MalformedResponse);
        }

        let sample = &self.sample;
        if !(1..=MAX_RECORD_MATCHES).contains(&sample.recent_matches.requested)
            || sample.recent_matches.returned != self.recent_matches.len()
            || sample.recent_matches.returned > sample.recent_matches.requested
            || sample.champion_aggregates.season_start_unix_seconds <= 0
            || sample.champion_aggregates.discovered_matches > MAX_SEASON_AGGREGATE_MATCHES
            || sample.champion_aggregates.analyzed_matches
                > sample.champion_aggregates.discovered_matches
        {
            return Err(RiotClientError::MalformedResponse);
        }

        let mut rank_queues = HashSet::with_capacity(self.ranks.len());
        if self.ranks.iter().any(|entry| {
            entry.queue_type.trim().is_empty()
                || entry.league_points < 0
                || entry.wins < 0
                || entry.losses < 0
                || !rank_queues.insert(entry.queue_type.as_str())
        }) {
            return Err(RiotClientError::MalformedResponse);
        }

        validate_recent_matches(&self.account.puuid, &self.recent_matches)?;

        let mut aggregate_champions = HashSet::with_capacity(self.champion_aggregates.len());
        let mut aggregate_games = 0_usize;
        if self.champion_aggregates.iter().any(|aggregate| {
            aggregate_games = aggregate_games.saturating_add(aggregate.games as usize);
            aggregate.champion_name.trim().is_empty()
                || aggregate.games == 0
                || aggregate.wins > aggregate.games
                || aggregate.kills < 0
                || aggregate.deaths < 0
                || aggregate.assists < 0
                || aggregate.creep_score < 0
                || aggregate.gold_earned < 0
                || aggregate.triple_kills < 0
                || aggregate.quadra_kills < 0
                || aggregate.penta_kills < 0
                || !aggregate_champions.insert(aggregate.champion_name.as_str())
        }) || aggregate_games != sample.champion_aggregates.analyzed_matches
        {
            return Err(RiotClientError::MalformedResponse);
        }
        if self.champion_aggregates.windows(2).any(|pair| {
            let left = &pair[0];
            let right = &pair[1];
            left.games < right.games
                || (left.games == right.games && left.wins < right.wins)
                || (left.games == right.games
                    && left.wins == right.wins
                    && left.champion_name > right.champion_name)
        }) {
            return Err(RiotClientError::MalformedResponse);
        }
        Ok(())
    }
}

fn validate_recent_matches(
    account_puuid: &str,
    matches: &[RiotRecentMatch],
) -> Result<(), RiotClientError> {
    let mut match_ids = HashSet::with_capacity(matches.len());
    for game in matches {
        if game.match_id.trim().is_empty()
            || game.game_version.trim().is_empty()
            || game.started_at.trim().is_empty()
            || game.duration_seconds < 0
            || game.target_puuid != account_puuid
            || game.participants.len() > MAX_MATCH_PARTICIPANTS
            || !match_ids.insert(game.match_id.as_str())
            || game.participants.iter().any(invalid_participant)
            || game.teams.iter().any(invalid_team)
        {
            return Err(RiotClientError::MalformedResponse);
        }

        let mut targets = game
            .participants
            .iter()
            .filter(|participant| participant.puuid == game.target_puuid);
        if targets.next().is_none() || targets.next().is_some() {
            return Err(RiotClientError::MalformedResponse);
        }
    }
    Ok(())
}

fn invalid_participant(participant: &RiotMatchParticipant) -> bool {
    participant.puuid.trim().is_empty()
        || participant.champion_name.trim().is_empty()
        || participant.champion_level < 0
        || participant.kills < 0
        || participant.deaths < 0
        || participant.assists < 0
        || participant.creep_score < 0
        || participant.gold_earned < 0
        || participant.damage_dealt_to_champions < 0
        || participant.damage_taken < 0
        || participant.vision_score < 0
        || participant.triple_kills < 0
        || participant.quadra_kills < 0
        || participant.penta_kills < 0
        || participant
            .summoner_spell_ids
            .iter()
            .chain(participant.item_ids.iter())
            .any(|value| *value < 0)
}

fn invalid_team(team: &RiotMatchTeam) -> bool {
    let objectives = &team.objectives;
    [
        objectives.baron_kills,
        objectives.champion_kills,
        objectives.dragon_kills,
        objectives.horde_kills,
        objectives.inhibitor_kills,
        objectives.rift_herald_kills,
        objectives.tower_kills,
    ]
    .into_iter()
    .any(|value| value < 0)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotRecordsSample {
    pub recent_matches: RiotRecentMatchSample,
    pub champion_aggregates: RiotChampionAggregateSample,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotRecentMatchSample {
    pub requested: usize,
    pub returned: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotChampionAggregateSample {
    pub season_start_unix_seconds: i64,
    pub discovered_matches: usize,
    pub analyzed_matches: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotRecentMatchesPage {
    pub account: RiotAccount,
    pub platform: RiotPlatform,
    pub routing_region: RiotRoutingRegion,
    pub target_puuid: String,
    pub matches: Vec<RiotRecentMatch>,
    pub page: RiotRecentMatchesPageMetadata,
}

impl RiotRecentMatchesPage {
    pub(crate) fn validate(
        &self,
        requested_platform: RiotPlatform,
        requested_offset: usize,
        requested_count: usize,
    ) -> Result<(), RiotClientError> {
        self.account.validate()?;
        let next_offset = requested_offset.checked_add(requested_count);
        if self.platform != requested_platform
            || self.routing_region != requested_platform.routing_region()
            || self.target_puuid != self.account.puuid
            || self.matches.len() > requested_count
            || self.page.offset != requested_offset
            || self.page.requested != requested_count
            || self.page.returned != self.matches.len()
            || self.page.returned > self.page.requested
            || (self.page.has_more && self.page.next_offset != next_offset)
            || (!self.page.has_more && self.page.next_offset.is_some())
        {
            return Err(RiotClientError::MalformedResponse);
        }
        validate_recent_matches(&self.account.puuid, &self.matches)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotRecentMatchesPageMetadata {
    pub offset: usize,
    pub requested: usize,
    pub returned: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotRank {
    pub queue_type: String,
    pub tier: Option<String>,
    pub rank: Option<String>,
    pub league_points: i32,
    pub wins: i32,
    pub losses: i32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotChampionAggregate {
    pub champion_name: String,
    pub games: u32,
    pub wins: u32,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub creep_score: i64,
    pub gold_earned: i64,
    #[serde(default)]
    pub triple_kills: i64,
    #[serde(default)]
    pub quadra_kills: i64,
    #[serde(default)]
    pub penta_kills: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotRecentMatch {
    pub match_id: String,
    pub queue_id: i32,
    pub game_version: String,
    pub duration_seconds: i64,
    pub started_at: String,
    pub target_puuid: String,
    pub participants: Vec<RiotMatchParticipant>,
    #[serde(default)]
    pub teams: Vec<RiotMatchTeam>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotMatchParticipant {
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
    #[serde(default)]
    pub triple_kills: i32,
    #[serde(default)]
    pub quadra_kills: i32,
    #[serde(default)]
    pub penta_kills: i32,
    pub team_position: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotMatchTeam {
    pub team_id: i32,
    pub win: bool,
    pub objectives: RiotTeamObjectives,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotTeamObjectives {
    pub baron_kills: i32,
    pub champion_kills: i32,
    pub dragon_kills: i32,
    pub horde_kills: i32,
    pub inhibitor_kills: i32,
    pub rift_herald_kills: i32,
    pub tower_kills: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotCacheMetadata {
    pub state: RiotCacheState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiotCacheState {
    Fresh,
    Stale,
    Refreshed,
}

#[derive(Debug, Error)]
pub enum RiotClientError {
    #[error("Riot integration configuration is invalid")]
    InvalidConfiguration,
    #[error("Riot integration service authentication failed")]
    ServiceUnauthorized,
    #[error("Riot request input is invalid")]
    InvalidInput,
    #[error("Riot platform is unsupported")]
    UnsupportedPlatform,
    #[error("Riot account was not found")]
    NotFound,
    #[error("Riot integration rate limit was reached")]
    RateLimited { retry_after_seconds: Option<u64> },
    #[error("Riot integration request timed out")]
    Timeout,
    #[error("Riot API credentials were rejected")]
    CredentialsRejected,
    #[error("Riot API is temporarily unavailable")]
    ProviderUnavailable,
    #[error("Riot API returned an unusable response")]
    ProviderBadGateway,
    #[error("Riot integration request failed")]
    Request(#[source] reqwest::Error),
    #[error("Riot integration rejected the request with HTTP {status}")]
    Rejected { status: u16 },
    #[error("Riot integration returned a malformed response")]
    MalformedResponse,
}

async fn decode_response<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, RiotClientError> {
    let status = response.status();
    if !status.is_success() {
        let retry_after_seconds = parse_retry_after(response.headers().get(RETRY_AFTER));
        let upstream_error = response.json::<InternalRiotErrorEnvelope>().await.ok();
        return Err(classify_error_response(
            status,
            retry_after_seconds,
            upstream_error.as_ref(),
        ));
    }
    response
        .json::<T>()
        .await
        .map_err(|_| RiotClientError::MalformedResponse)
}

#[derive(Debug, Deserialize)]
struct InternalRiotErrorEnvelope {
    error: InternalRiotErrorData,
}

#[derive(Debug, Deserialize)]
struct InternalRiotErrorData {
    code: String,
}

fn classify_error_response(
    status: StatusCode,
    retry_after_seconds: Option<u64>,
    upstream_error: Option<&InternalRiotErrorEnvelope>,
) -> RiotClientError {
    match upstream_error.map(|value| value.error.code.as_str()) {
        Some("unauthorized") => RiotClientError::ServiceUnauthorized,
        Some("invalid_request") => RiotClientError::InvalidInput,
        Some("riot_account_not_found") => RiotClientError::NotFound,
        Some("riot_rate_limited") => RiotClientError::RateLimited {
            retry_after_seconds,
        },
        Some("riot_timeout") => RiotClientError::Timeout,
        Some("riot_credentials_rejected") => RiotClientError::CredentialsRejected,
        Some("riot_unavailable") => RiotClientError::ProviderUnavailable,
        Some("riot_bad_gateway") => RiotClientError::ProviderBadGateway,
        _ => match status {
            StatusCode::NOT_FOUND => RiotClientError::NotFound,
            StatusCode::TOO_MANY_REQUESTS => RiotClientError::RateLimited {
                retry_after_seconds,
            },
            _ => RiotClientError::Rejected {
                status: status.as_u16(),
            },
        },
    }
}

fn map_request_error(error: reqwest::Error) -> RiotClientError {
    if error.is_timeout() {
        RiotClientError::Timeout
    } else {
        RiotClientError::Request(error)
    }
}

fn parse_retry_after(value: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    value?.to_str().ok()?.parse().ok()
}

fn validate_riot_id_component(value: &str, max_chars: usize) -> Result<String, RiotClientError> {
    let value = value.trim();
    let count = value.chars().count();
    if count == 0 || count > max_chars || value.contains('#') || value.chars().any(char::is_control)
    {
        return Err(RiotClientError::InvalidInput);
    }
    Ok(value.to_owned())
}

fn validate_puuid(value: &str) -> Result<(), RiotClientError> {
    if value.is_empty()
        || value.len() > MAX_PUUID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RiotClientError::InvalidInput);
    }
    Ok(())
}

fn is_internal_service_base_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().ends_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_mapping_is_allowlisted_and_typed() {
        assert_eq!("kr".parse::<RiotPlatform>().unwrap(), RiotPlatform::Kr);
        assert_eq!("KR".parse::<RiotPlatform>().unwrap(), RiotPlatform::Kr);
        assert_eq!(RiotPlatform::Kr.as_str(), "KR");
        assert_eq!(RiotPlatform::Kr.routing_region().as_str(), "ASIA");
        assert_eq!(
            "na1".parse::<RiotPlatform>().unwrap().routing_region(),
            RiotRoutingRegion::Americas
        );
        assert!("moon1".parse::<RiotPlatform>().is_err());
    }

    #[test]
    fn riot_id_validation_trims_but_rejects_ambiguous_components() {
        let riot_id = RiotId::new(" Hide on bush ", " KR1 ").unwrap();
        assert_eq!(riot_id.game_name, "Hide on bush");
        assert_eq!(riot_id.tag_line, "KR1");

        for (game_name, tag_line) in [("", "KR1"), ("Name#Other", "KR1"), ("Name", "K\nR")] {
            assert!(RiotId::new(game_name, tag_line).is_err());
        }
    }

    #[test]
    fn internal_base_url_must_be_an_http_directory_url() {
        assert!(is_internal_service_base_url(
            &Url::parse("http://riot-api:8081/").unwrap()
        ));
        assert!(is_internal_service_base_url(
            &Url::parse("https://internal.example/riot/").unwrap()
        ));
        assert!(!is_internal_service_base_url(
            &Url::parse("https://user:pass@internal.example/").unwrap()
        ));
        assert!(!is_internal_service_base_url(
            &Url::parse("https://internal.example/no-trailing-slash").unwrap()
        ));
    }

    #[test]
    fn account_lookup_contract_is_explicitly_tagged() {
        let request = ResolveAccountRequest {
            platform: RiotPlatform::Kr,
            lookup: AccountLookup::RiotId {
                game_name: "Longcat".to_owned(),
                tag_line: "KR1".to_owned(),
            },
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "platform": "KR",
                "lookup": {"kind": "riotId", "gameName": "Longcat", "tagLine": "KR1"}
            })
        );
    }

    #[test]
    fn retry_after_accepts_only_delta_seconds() {
        let seconds = reqwest::header::HeaderValue::from_static("12");
        let date = reqwest::header::HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(parse_retry_after(Some(&seconds)), Some(12));
        assert_eq!(parse_retry_after(Some(&date)), None);
        assert_eq!(parse_retry_after(None), None);
    }

    #[test]
    fn internal_error_envelope_preserves_safe_riot_failure_cause() {
        let envelope = |code: &str| InternalRiotErrorEnvelope {
            error: InternalRiotErrorData {
                code: code.to_owned(),
            },
        };

        assert!(matches!(
            classify_error_response(
                StatusCode::UNAUTHORIZED,
                None,
                Some(&envelope("unauthorized"))
            ),
            RiotClientError::ServiceUnauthorized
        ));
        assert!(matches!(
            classify_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                None,
                Some(&envelope("riot_credentials_rejected"))
            ),
            RiotClientError::CredentialsRejected
        ));
        assert!(matches!(
            classify_error_response(
                StatusCode::TOO_MANY_REQUESTS,
                Some(7),
                Some(&envelope("riot_rate_limited"))
            ),
            RiotClientError::RateLimited {
                retry_after_seconds: Some(7)
            }
        ));
        assert!(matches!(
            classify_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                None,
                Some(&envelope("riot_unavailable"))
            ),
            RiotClientError::ProviderUnavailable
        ));
        assert!(matches!(
            classify_error_response(StatusCode::BAD_GATEWAY, None, Some(&envelope("unknown"))),
            RiotClientError::Rejected { status: 502 }
        ));
    }

    #[test]
    fn records_wire_contract_preserves_page_fields_and_requires_every_sample_aggregate() {
        let payload = serde_json::json!({
            "account": {"puuid":"target-puuid","gameName":"Longcat","tagLine":"KR1"},
            "platform": "KR",
            "routingRegion": "ASIA",
            "profileIconId": 6668,
            "summonerLevel": 258,
            "ranks": [{
                "queueType":"RANKED_SOLO_5x5","tier":"PLATINUM","rank":"III",
                "leaguePoints":23,"wins":22,"losses":29
            }],
            "championAggregates": [{
                "championName":"LeeSin","games":1,"wins":1,"kills":10,"deaths":2,
                "assists":8,"creepScore":220,"goldEarned":14000,
                "tripleKills":1,"quadraKills":1,"pentaKills":1
            }],
            "recentMatches": [{
                "matchId":"KR_1","queueId":420,"gameVersion":"16.13.1",
                "durationSeconds":1800,"startedAt":"2026-07-13T00:00:00Z",
                "targetPuuid":"target-puuid",
                "participants": [{
                    "puuid":"target-puuid","gameName":"Longcat","tagLine":"KR1",
                    "championName":"LeeSin","championLevel":18,"teamId":100,"win":true,
                    "kills":10,"deaths":2,"assists":8,"creepScore":220,"goldEarned":14000,
                    "damageDealtToChampions":30000,"damageTaken":20000,"visionScore":40,
                    "summonerSpellIds":[4,11],"itemIds":[1001,1002,1003,1004,1005,1006,3364],
                    "tripleKills":1,"quadraKills":1,"pentaKills":1,"teamPosition":"JUNGLE"
                }],
                "teams": [{
                    "teamId":100,"win":true,"objectives":{
                        "baronKills":1,"championKills":40,"dragonKills":3,"hordeKills":3,
                        "inhibitorKills":2,"riftHeraldKills":1,"towerKills":11
                    }
                }]
            }],
            "sample": {
                "recentMatches": {"requested":10,"returned":1},
                "championAggregates": {
                    "seasonStartUnixSeconds":1735689600,
                    "discoveredMatches":1,"analyzedMatches":1,"truncated":false
                }
            },
            "refreshedAt":"2026-07-13T00:00:00Z",
            "cache":{"state":"fresh"}
        });
        let records = serde_json::from_value::<RiotRecords>(payload.clone()).expect("records DTO");
        assert!(records.validate(RiotPlatform::Kr).is_ok());
        assert_eq!(records.ranks[0].league_points, 23);
        assert_eq!(
            records.recent_matches[0].teams[0].objectives.tower_kills,
            11
        );
        assert_eq!(records.champion_aggregates[0].penta_kills, 1);

        let mut page = RiotRecentMatchesPage {
            account: records.account.clone(),
            platform: records.platform,
            routing_region: records.routing_region,
            target_puuid: records.account.puuid.clone(),
            matches: records.recent_matches.clone(),
            page: RiotRecentMatchesPageMetadata {
                offset: 10,
                requested: 10,
                returned: 1,
                has_more: true,
                next_offset: Some(20),
            },
        };
        assert!(page.validate(RiotPlatform::Kr, 10, 10).is_ok());
        page.page.next_offset = Some(21);
        assert!(matches!(
            page.validate(RiotPlatform::Kr, 10, 10),
            Err(RiotClientError::MalformedResponse)
        ));

        let mut missing_aggregate = payload;
        missing_aggregate["championAggregates"] = serde_json::json!([]);
        let records =
            serde_json::from_value::<RiotRecords>(missing_aggregate).expect("records DTO shape");
        assert!(matches!(
            records.validate(RiotPlatform::Kr),
            Err(RiotClientError::MalformedResponse)
        ));
    }
}
