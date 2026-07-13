use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::{StreamExt, stream};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::{Mutex, RwLock};

use crate::{
    client::{MatchDto, MatchIdsQuery, MatchType, RiotAccountDto, RiotClient, RiotClientError},
    config::MAX_MATCH_COUNT,
    domain::{
        AccountIdentity, CacheMetadata, CacheState, ChampionAggregate,
        ChampionAggregateSampleMetadata, InputError, MatchParticipant, MatchSummary, MatchTeam,
        Platform, RankEntry, RecentMatchSampleMetadata, RecentMatchesPage,
        RecentMatchesPageMetadata, RecordsSampleMetadata, RecordsSnapshot, ResolvedAccount,
        RiotIdInput, TeamObjectives, ValidatedRiotId,
    },
};

const FRESH_TTL: Duration = Duration::from_secs(5 * 60);
const STALE_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_CACHE_ENTRIES: usize = 1_024;
const MATCH_CONCURRENCY: usize = 4;
const SEASON_MATCH_PAGE_SIZE: usize = 100;
const MAX_SEASON_MATCH_PAGES: usize = 20;
const MAX_SEASON_MATCHES: usize = SEASON_MATCH_PAGE_SIZE * MAX_SEASON_MATCH_PAGES;
pub(crate) const DEFAULT_RECENT_MATCH_BATCH: usize = 10;
pub(crate) const MAX_RECENT_MATCH_BATCH: usize = 20;
const MAX_RECENT_MATCH_OFFSET: usize = 10_000;

pub struct RiotService {
    client: Arc<dyn RiotClient>,
    match_count: usize,
    season_start_unix_seconds: i64,
    cache: RwLock<HashMap<CacheKey, CachedSnapshot>>,
    in_flight: Mutex<HashMap<CacheKey, Arc<Mutex<()>>>>,
}

impl RiotService {
    pub fn new(
        client: Arc<dyn RiotClient>,
        match_count: usize,
        season_start_unix_seconds: i64,
    ) -> Self {
        Self {
            client,
            match_count: match_count.clamp(1, MAX_MATCH_COUNT),
            season_start_unix_seconds,
            cache: RwLock::new(HashMap::new()),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    pub async fn resolve_by_riot_id(
        &self,
        input: RiotIdInput,
    ) -> Result<ResolvedAccount, ServiceError> {
        let input = input.validate()?;
        let routing = input.platform.routing();
        let account = self
            .client
            .account_by_riot_id(routing, &input.game_name, &input.tag_line)
            .await?;
        self.hydrate_account(account, input.platform).await
    }

    pub async fn resolve_by_puuid(
        &self,
        platform: &str,
        puuid: &str,
    ) -> Result<ResolvedAccount, ServiceError> {
        let platform = Platform::parse(platform).map_err(|_| InputError::Platform)?;
        let puuid = validate_puuid(puuid)?;
        let account = self
            .client
            .account_by_puuid(platform.routing(), puuid)
            .await?;
        self.hydrate_account(account, platform).await
    }

    pub async fn records(
        &self,
        input: RiotIdInput,
        force_refresh: bool,
    ) -> Result<RecordsSnapshot, ServiceError> {
        let input = input.validate()?;
        let key = CacheKey::from(&input);

        if !force_refresh && let Some(snapshot) = self.fresh_cache(&key).await {
            return Ok(snapshot);
        }

        let request_lock = {
            let mut in_flight = self.in_flight.lock().await;
            in_flight
                .entry(key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let guard = request_lock.lock().await;

        if !force_refresh && let Some(snapshot) = self.fresh_cache(&key).await {
            drop(guard);
            self.remove_request_lock(&key, &request_lock).await;
            return Ok(snapshot);
        }

        let stale = self.stale_cache(&key).await;
        let result = self.fetch_records(input).await;
        let result = match result {
            Ok(mut snapshot) => {
                snapshot.cache.state = if force_refresh {
                    CacheState::Refreshed
                } else {
                    CacheState::Fresh
                };
                self.store_cache(key.clone(), snapshot.clone()).await;
                Ok(snapshot)
            }
            Err(error) if error.can_use_stale() => stale.ok_or(error).map(|mut snapshot| {
                snapshot.cache.state = CacheState::Stale;
                snapshot
            }),
            Err(error) => Err(error),
        };

        drop(guard);
        self.remove_request_lock(&key, &request_lock).await;
        result
    }

    pub async fn recent_matches_page(
        &self,
        input: RiotIdInput,
        offset: usize,
        count: usize,
    ) -> Result<RecentMatchesPage, ServiceError> {
        if !(1..=MAX_RECENT_MATCH_BATCH).contains(&count) || offset > MAX_RECENT_MATCH_OFFSET {
            return Err(InputError::Pagination.into());
        }
        let input = input.validate()?;
        let routing = input.platform.routing();
        let account = self
            .client
            .account_by_riot_id(routing, &input.game_name, &input.tag_line)
            .await?;
        let target_puuid = account.puuid.clone();
        let match_ids = self
            .client
            .match_ids(
                routing,
                &target_puuid,
                MatchIdsQuery {
                    start: offset,
                    count: count + 1,
                    start_time: None,
                    match_type: None,
                },
            )
            .await?;
        let mut match_ids = ordered_unique(match_ids.into_iter().take(count + 1));
        let has_more = match_ids.len() > count;
        match_ids.truncate(count);
        let details = self
            .fetch_match_details(routing, &target_puuid, match_ids.clone())
            .await?;
        let matches = match_ids
            .iter()
            .filter_map(|match_id| details.get(match_id).cloned())
            .collect::<Vec<_>>();

        Ok(RecentMatchesPage {
            account: map_account(account),
            platform: input.platform.code().to_owned(),
            routing_region: routing.code().to_owned(),
            target_puuid,
            page: RecentMatchesPageMetadata {
                offset,
                requested: count,
                returned: matches.len(),
                has_more,
                next_offset: has_more.then_some(offset + count),
            },
            matches,
        })
    }

    async fn hydrate_account(
        &self,
        account: RiotAccountDto,
        platform: Platform,
    ) -> Result<ResolvedAccount, ServiceError> {
        let summoner = self
            .client
            .summoner_by_puuid(platform, &account.puuid)
            .await?;
        Ok(ResolvedAccount {
            account: map_account(account),
            platform: platform.code().to_owned(),
            routing_region: platform.routing().code().to_owned(),
            profile_icon_id: summoner.profile_icon_id,
            summoner_level: summoner.summoner_level,
            synced_at: now_rfc3339(),
        })
    }

    async fn fetch_records(&self, input: ValidatedRiotId) -> Result<RecordsSnapshot, ServiceError> {
        let routing = input.platform.routing();
        let account = self
            .client
            .account_by_riot_id(routing, &input.game_name, &input.tag_line)
            .await?;
        let puuid = account.puuid.clone();
        let (summoner, ranks, recent_match_ids, season_match_ids) = tokio::try_join!(
            self.client.summoner_by_puuid(input.platform, &puuid),
            self.client.league_entries_by_puuid(input.platform, &puuid),
            self.client.match_ids(
                routing,
                &puuid,
                MatchIdsQuery {
                    start: 0,
                    count: self.match_count,
                    start_time: None,
                    match_type: None,
                },
            ),
            self.fetch_season_match_ids(routing, &puuid),
        )?;

        // Treat provider arrays as untrusted even when a smaller count was requested.
        let recent_match_ids = ordered_unique(recent_match_ids.into_iter().take(self.match_count));
        let detail_ids = ordered_unique(
            recent_match_ids
                .iter()
                .chain(&season_match_ids.ids)
                .cloned(),
        );
        let details = self
            .fetch_match_details(routing, &puuid, detail_ids)
            .await?;

        let recent_matches = recent_match_ids
            .iter()
            .filter_map(|match_id| details.get(match_id).cloned())
            .collect::<Vec<_>>();
        let season_matches = season_match_ids
            .ids
            .iter()
            .filter_map(|match_id| details.get(match_id))
            .collect::<Vec<_>>();
        let champion_aggregates = aggregate_champions(season_matches.iter().copied(), &puuid);

        let mut ranks = ranks
            .into_iter()
            .map(|entry| RankEntry {
                queue_type: entry.queue_type,
                tier: entry.tier,
                rank: entry.rank,
                league_points: entry.league_points,
                wins: entry.wins,
                losses: entry.losses,
            })
            .collect::<Vec<_>>();
        ranks.sort_by(|left, right| {
            rank_queue_priority(&left.queue_type)
                .cmp(&rank_queue_priority(&right.queue_type))
                .then_with(|| left.queue_type.cmp(&right.queue_type))
        });

        Ok(RecordsSnapshot {
            account: map_account(account),
            platform: input.platform.code().to_owned(),
            routing_region: routing.code().to_owned(),
            profile_icon_id: summoner.profile_icon_id,
            summoner_level: summoner.summoner_level,
            ranks,
            champion_aggregates,
            recent_matches,
            sample: RecordsSampleMetadata {
                recent_matches: RecentMatchSampleMetadata {
                    requested: self.match_count,
                    returned: recent_match_ids
                        .iter()
                        .filter(|match_id| details.contains_key(*match_id))
                        .count(),
                },
                champion_aggregates: ChampionAggregateSampleMetadata {
                    season_start_unix_seconds: self.season_start_unix_seconds,
                    discovered_matches: season_match_ids.ids.len(),
                    analyzed_matches: season_matches.len(),
                    truncated: season_match_ids.truncated,
                },
            },
            refreshed_at: now_rfc3339(),
            cache: CacheMetadata {
                state: CacheState::Fresh,
            },
        })
    }

    async fn fetch_season_match_ids(
        &self,
        routing: crate::domain::RoutingRegion,
        puuid: &str,
    ) -> Result<SeasonMatchIds, RiotClientError> {
        let mut ids = Vec::with_capacity(MAX_SEASON_MATCHES);
        let mut seen = HashSet::with_capacity(MAX_SEASON_MATCHES);

        for page_index in 0..MAX_SEASON_MATCH_PAGES {
            let page = self
                .client
                .match_ids(
                    routing,
                    puuid,
                    MatchIdsQuery {
                        start: page_index * SEASON_MATCH_PAGE_SIZE,
                        count: SEASON_MATCH_PAGE_SIZE,
                        start_time: Some(self.season_start_unix_seconds),
                        match_type: Some(MatchType::Ranked),
                    },
                )
                .await?;
            let returned = page.len();
            for match_id in page.into_iter().take(SEASON_MATCH_PAGE_SIZE) {
                if seen.insert(match_id.clone()) {
                    ids.push(match_id);
                }
            }

            if returned < SEASON_MATCH_PAGE_SIZE {
                return Ok(SeasonMatchIds {
                    ids,
                    truncated: false,
                });
            }
            if returned > SEASON_MATCH_PAGE_SIZE {
                return Ok(SeasonMatchIds {
                    ids,
                    truncated: true,
                });
            }
            if ids.len() >= MAX_SEASON_MATCHES {
                let overflow = self
                    .client
                    .match_ids(
                        routing,
                        puuid,
                        MatchIdsQuery {
                            start: MAX_SEASON_MATCHES,
                            count: 1,
                            start_time: Some(self.season_start_unix_seconds),
                            match_type: Some(MatchType::Ranked),
                        },
                    )
                    .await?;
                return Ok(SeasonMatchIds {
                    ids,
                    truncated: !overflow.is_empty(),
                });
            }
        }

        Ok(SeasonMatchIds {
            ids,
            truncated: true,
        })
    }

    async fn fetch_match_details(
        &self,
        routing: crate::domain::RoutingRegion,
        target_puuid: &str,
        match_ids: Vec<String>,
    ) -> Result<HashMap<String, MatchSummary>, RiotClientError> {
        let client = self.client.clone();
        let target_puuid = target_puuid.to_owned();
        let mut pending = stream::iter(match_ids.into_iter().map(|match_id| {
            let client = client.clone();
            let target_puuid = target_puuid.clone();
            async move {
                let result = client.match_detail(routing, &match_id).await;
                (match_id, target_puuid, result)
            }
        }))
        .buffer_unordered(MATCH_CONCURRENCY);
        let mut details = HashMap::new();

        while let Some((requested_match_id, target_puuid, result)) = pending.next().await {
            match result {
                Ok(detail) => {
                    if detail.metadata.match_id != requested_match_id {
                        return Err(RiotClientError::MalformedResponse);
                    }
                    let summary = map_match(detail, &target_puuid)?;
                    details.insert(requested_match_id, summary);
                }
                Err(RiotClientError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(details)
    }

    async fn fresh_cache(&self, key: &CacheKey) -> Option<RecordsSnapshot> {
        let cache = self.cache.read().await;
        let cached = cache.get(key)?;
        (cached.stored_at.elapsed() <= FRESH_TTL).then(|| {
            let mut snapshot = cached.snapshot.clone();
            snapshot.cache.state = CacheState::Fresh;
            snapshot
        })
    }

    async fn stale_cache(&self, key: &CacheKey) -> Option<RecordsSnapshot> {
        let cache = self.cache.read().await;
        let cached = cache.get(key)?;
        (cached.stored_at.elapsed() <= STALE_TTL).then(|| cached.snapshot.clone())
    }

    async fn store_cache(&self, key: CacheKey, snapshot: RecordsSnapshot) {
        let mut cache = self.cache.write().await;
        if cache.len() >= MAX_CACHE_ENTRIES
            && !cache.contains_key(&key)
            && let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, value)| value.stored_at)
                .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
        cache.insert(
            key,
            CachedSnapshot {
                snapshot,
                stored_at: Instant::now(),
            },
        );
    }

    async fn remove_request_lock(&self, key: &CacheKey, request_lock: &Arc<Mutex<()>>) {
        let mut in_flight = self.in_flight.lock().await;
        if in_flight
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, request_lock))
        {
            in_flight.remove(key);
        }
    }
}

fn map_account(account: RiotAccountDto) -> AccountIdentity {
    AccountIdentity {
        puuid: account.puuid,
        game_name: account.game_name,
        tag_line: account.tag_line,
    }
}

fn map_match(detail: MatchDto, target_puuid: &str) -> Result<MatchSummary, RiotClientError> {
    let summary = MatchSummary {
        match_id: detail.metadata.match_id,
        queue_id: detail.info.queue_id,
        game_version: detail.info.game_version,
        duration_seconds: detail.info.game_duration,
        started_at: timestamp_millis_rfc3339(detail.info.game_creation),
        target_puuid: target_puuid.to_owned(),
        participants: detail
            .info
            .participants
            .into_iter()
            .map(|participant| MatchParticipant {
                puuid: participant.puuid,
                game_name: participant.riot_id_game_name,
                tag_line: participant.riot_id_tagline,
                champion_name: participant.champion_name,
                champion_level: participant.champ_level,
                team_id: participant.team_id,
                win: participant.win,
                kills: participant.kills,
                deaths: participant.deaths,
                assists: participant.assists,
                creep_score: participant.total_minions_killed + participant.neutral_minions_killed,
                gold_earned: participant.gold_earned,
                damage_dealt_to_champions: participant.total_damage_dealt_to_champions,
                damage_taken: participant.total_damage_taken,
                vision_score: participant.vision_score,
                summoner_spell_ids: [participant.summoner1_id, participant.summoner2_id],
                item_ids: [
                    participant.item0,
                    participant.item1,
                    participant.item2,
                    participant.item3,
                    participant.item4,
                    participant.item5,
                    participant.item6,
                ],
                triple_kills: participant.triple_kills,
                quadra_kills: participant.quadra_kills,
                penta_kills: participant.penta_kills,
                team_position: participant.team_position,
            })
            .collect(),
        teams: detail
            .info
            .teams
            .into_iter()
            .map(|team| MatchTeam {
                team_id: team.team_id,
                win: team.win,
                objectives: TeamObjectives {
                    baron_kills: team.objectives.baron.kills,
                    champion_kills: team.objectives.champion.kills,
                    dragon_kills: team.objectives.dragon.kills,
                    horde_kills: team.objectives.horde.kills,
                    inhibitor_kills: team.objectives.inhibitor.kills,
                    rift_herald_kills: team.objectives.rift_herald.kills,
                    tower_kills: team.objectives.tower.kills,
                },
            })
            .collect(),
    };
    if summary
        .participants
        .iter()
        .filter(|participant| participant.puuid == target_puuid)
        .count()
        != 1
    {
        return Err(RiotClientError::MalformedResponse);
    }
    Ok(summary)
}

fn aggregate_champions<'a>(
    matches: impl IntoIterator<Item = &'a MatchSummary>,
    target_puuid: &str,
) -> Vec<ChampionAggregate> {
    let mut aggregates: HashMap<String, ChampionAggregate> = HashMap::new();
    for participant in matches
        .into_iter()
        .filter_map(|game| game.participants.iter().find(|p| p.puuid == target_puuid))
    {
        let aggregate = aggregates
            .entry(participant.champion_name.clone())
            .or_insert_with(|| ChampionAggregate {
                champion_name: participant.champion_name.clone(),
                games: 0,
                wins: 0,
                kills: 0,
                deaths: 0,
                assists: 0,
                creep_score: 0,
                gold_earned: 0,
                triple_kills: 0,
                quadra_kills: 0,
                penta_kills: 0,
            });
        aggregate.games += 1;
        aggregate.wins += u32::from(participant.win);
        aggregate.kills += i64::from(participant.kills);
        aggregate.deaths += i64::from(participant.deaths);
        aggregate.assists += i64::from(participant.assists);
        aggregate.creep_score += i64::from(participant.creep_score);
        aggregate.gold_earned += i64::from(participant.gold_earned);
        aggregate.triple_kills += i64::from(participant.triple_kills);
        aggregate.quadra_kills += i64::from(participant.quadra_kills);
        aggregate.penta_kills += i64::from(participant.penta_kills);
    }
    let mut values: Vec<_> = aggregates.into_values().collect();
    values.sort_by(|left, right| {
        right
            .games
            .cmp(&left.games)
            .then_with(|| right.wins.cmp(&left.wins))
            .then_with(|| left.champion_name.cmp(&right.champion_name))
    });
    values
}

fn ordered_unique(match_ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    match_ids
        .into_iter()
        .filter(|match_id| seen.insert(match_id.clone()))
        .collect()
}

fn rank_queue_priority(queue_type: &str) -> u8 {
    match queue_type {
        "RANKED_SOLO_5x5" => 0,
        "RANKED_FLEX_SR" => 1,
        _ => 2,
    }
}

fn validate_puuid(value: &str) -> Result<&str, InputError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(InputError::RiotId);
    }
    Ok(value)
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn timestamp_millis_rfc3339(value: i64) -> String {
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp_nanos(i128::from(value) * 1_000_000)
    else {
        return "1970-01-01T00:00:00Z".to_owned();
    };
    timestamp
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    platform: Platform,
    game_name: String,
    tag_line: String,
}

impl From<&ValidatedRiotId> for CacheKey {
    fn from(input: &ValidatedRiotId) -> Self {
        Self {
            platform: input.platform,
            game_name: input.game_name.to_lowercase(),
            tag_line: input.tag_line.to_lowercase(),
        }
    }
}

struct CachedSnapshot {
    snapshot: RecordsSnapshot,
    stored_at: Instant,
}

struct SeasonMatchIds {
    ids: Vec<String>,
    truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Input(#[from] InputError),
    #[error(transparent)]
    Riot(#[from] RiotClientError),
}

impl ServiceError {
    fn can_use_stale(&self) -> bool {
        matches!(
            self,
            Self::Riot(
                RiotClientError::RateLimited { .. }
                    | RiotClientError::Timeout
                    | RiotClientError::Unavailable
                    | RiotClientError::MalformedResponse
            )
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

    use futures_util::future::BoxFuture;

    use super::*;
    use crate::client::{
        LeagueEntryDto, MatchInfoDto, MatchMetadataDto, MatchObjectiveDto, MatchObjectivesDto,
        MatchParticipantDto, MatchTeamDto, SummonerDto,
    };
    use crate::domain::RoutingRegion;

    const SEASON_START: i64 = 1_735_689_600;
    const FULL_SEASON_MATCHES: usize = 135;

    #[tokio::test]
    async fn records_separate_recent_sample_from_complete_sorted_season_aggregates() {
        for sample_size in [10, 20] {
            let client = Arc::new(SampleClient::new(FULL_SEASON_MATCHES));
            let service = RiotService::new(client.clone(), sample_size, SEASON_START);

            let snapshot = service
                .records(riot_id(), false)
                .await
                .expect("records snapshot");

            assert_eq!(
                client.requested_match_count.load(Ordering::SeqCst),
                sample_size
            );
            assert_eq!(snapshot.recent_matches.len(), sample_size);
            assert_eq!(snapshot.sample.recent_matches.requested, sample_size);
            assert_eq!(snapshot.sample.recent_matches.returned, sample_size);
            assert_eq!(
                snapshot
                    .sample
                    .champion_aggregates
                    .season_start_unix_seconds,
                SEASON_START
            );
            assert_eq!(
                snapshot.sample.champion_aggregates.discovered_matches,
                FULL_SEASON_MATCHES
            );
            assert_eq!(
                snapshot.sample.champion_aggregates.analyzed_matches,
                FULL_SEASON_MATCHES
            );
            assert!(!snapshot.sample.champion_aggregates.truncated);
            assert_eq!(client.season_page_requests.load(Ordering::SeqCst), 2);
            assert_eq!(client.season_start.load(Ordering::SeqCst), SEASON_START);
            assert!(client.season_ranked_only.load(Ordering::SeqCst));

            assert_eq!(snapshot.champion_aggregates.len(), 6);
            assert_eq!(
                snapshot
                    .champion_aggregates
                    .iter()
                    .map(|aggregate| aggregate.games)
                    .sum::<u32>(),
                FULL_SEASON_MATCHES as u32
            );
            assert_eq!(
                snapshot
                    .champion_aggregates
                    .iter()
                    .map(|aggregate| (aggregate.champion_name.as_str(), aggregate.games))
                    .collect::<Vec<_>>(),
                vec![
                    ("Ahri", 40),
                    ("LeeSin", 30),
                    ("Gnar", 25),
                    ("Lux", 20),
                    ("Zed", 10),
                    ("Vi", 10),
                ]
            );
            let ahri = snapshot
                .champion_aggregates
                .iter()
                .find(|aggregate| aggregate.champion_name == "Ahri")
                .expect("Ahri aggregate");
            assert_eq!(ahri.triple_kills, 40);
            assert_eq!(ahri.quadra_kills, 40);
            assert_eq!(ahri.penta_kills, 40);

            let expected_match_ids = (0..sample_size)
                .map(|index| format!("GEN1-MATCH{index:03}"))
                .collect::<Vec<_>>();
            assert_eq!(
                snapshot
                    .recent_matches
                    .iter()
                    .map(|game| game.match_id.clone())
                    .collect::<Vec<_>>(),
                expected_match_ids
            );

            let first_match = &snapshot.recent_matches[0];
            assert_eq!(first_match.participants[0].triple_kills, 1);
            assert_eq!(first_match.participants[0].quadra_kills, 1);
            assert_eq!(first_match.participants[0].penta_kills, 1);
            assert_eq!(first_match.teams.len(), 2);
            assert_eq!(first_match.teams[0].objectives.tower_kills, 11);
            assert_eq!(first_match.teams[0].objectives.dragon_kills, 3);

            assert_eq!(snapshot.ranks.len(), 2);
            assert_eq!(snapshot.ranks[0].queue_type, "RANKED_SOLO_5x5");
            assert_eq!(snapshot.ranks[0].tier.as_deref(), Some("PLATINUM"));
            assert_eq!(snapshot.ranks[0].rank.as_deref(), Some("III"));
            assert_eq!(snapshot.ranks[0].league_points, 23);
            assert_eq!(snapshot.ranks[0].wins, 22);
            assert_eq!(snapshot.ranks[0].losses, 29);
            assert_eq!(snapshot.ranks[1].queue_type, "RANKED_FLEX_SR");
        }
    }

    #[tokio::test]
    async fn refresh_bypasses_fresh_cache_then_becomes_the_authoritative_cached_snapshot() {
        let client = Arc::new(SampleClient::new(5));
        let service = RiotService::new(client.clone(), 3, SEASON_START);

        let initial = service
            .records(riot_id(), false)
            .await
            .expect("initial fetch");
        assert_eq!(initial.cache.state, CacheState::Fresh);
        assert_eq!(initial.recent_matches[0].match_id, "GEN1-MATCH000");

        let cached = service
            .records(riot_id(), false)
            .await
            .expect("fresh cache");
        assert_eq!(cached.cache.state, CacheState::Fresh);
        assert_eq!(client.provider_fetches.load(Ordering::SeqCst), 1);

        let refreshed = service
            .records(riot_id(), true)
            .await
            .expect("forced refresh");
        assert_eq!(refreshed.cache.state, CacheState::Refreshed);
        assert_eq!(refreshed.recent_matches[0].match_id, "GEN2-MATCH000");
        assert_eq!(client.provider_fetches.load(Ordering::SeqCst), 2);

        let refreshed_cache = service
            .records(riot_id(), false)
            .await
            .expect("refreshed cache");
        assert_eq!(refreshed_cache.cache.state, CacheState::Fresh);
        assert_eq!(refreshed_cache.recent_matches[0].match_id, "GEN2-MATCH000");
        assert_eq!(client.provider_fetches.load(Ordering::SeqCst), 2);

        client.fail_provider.store(true, Ordering::SeqCst);
        let stale = service
            .records(riot_id(), true)
            .await
            .expect("stale fallback");
        assert_eq!(stale.cache.state, CacheState::Stale);
        assert_eq!(stale.recent_matches[0].match_id, "GEN2-MATCH000");
        assert_eq!(client.provider_fetches.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn recent_match_pages_use_all_queues_and_return_stable_next_offsets() {
        let client = Arc::new(SampleClient::new(35));
        let service = RiotService::new(client.clone(), 10, SEASON_START);

        let page = service
            .recent_matches_page(riot_id(), 10, 10)
            .await
            .expect("middle recent page");
        assert_eq!(page.account.puuid, "target-puuid");
        assert_eq!(page.target_puuid, "target-puuid");
        assert_eq!(page.matches.len(), 10);
        assert_eq!(page.matches[0].match_id, "GEN1-MATCH010");
        assert_eq!(page.matches[9].match_id, "GEN1-MATCH019");
        assert_eq!(page.page.offset, 10);
        assert_eq!(page.page.requested, 10);
        assert_eq!(page.page.returned, 10);
        assert!(page.page.has_more);
        assert_eq!(page.page.next_offset, Some(20));
        assert_eq!(client.recent_start.load(Ordering::SeqCst), 10);
        assert!(client.recent_all_queues.load(Ordering::SeqCst));

        let last_page = service
            .recent_matches_page(riot_id(), 30, 10)
            .await
            .expect("last recent page");
        assert_eq!(last_page.matches.len(), 5);
        assert!(!last_page.page.has_more);
        assert_eq!(last_page.page.next_offset, None);

        assert!(matches!(
            service.recent_matches_page(riot_id(), 0, 21).await,
            Err(ServiceError::Input(InputError::Pagination))
        ));
    }

    #[tokio::test]
    async fn season_aggregate_scan_is_bounded_and_reports_truncation() {
        let exact_client = Arc::new(SampleClient::new(MAX_SEASON_MATCHES));
        let exact_service = RiotService::new(exact_client, 10, SEASON_START);
        let exact = exact_service
            .fetch_season_match_ids(RoutingRegion::Asia, "target-puuid")
            .await
            .expect("exact cap ID scan");
        assert_eq!(exact.ids.len(), MAX_SEASON_MATCHES);
        assert!(!exact.truncated);

        let client = Arc::new(SampleClient::new(MAX_SEASON_MATCHES + 1));
        let service = RiotService::new(client, 10, SEASON_START);

        let snapshot = service
            .records(riot_id(), false)
            .await
            .expect("bounded season snapshot");
        assert_eq!(
            snapshot.sample.champion_aggregates.discovered_matches,
            MAX_SEASON_MATCHES
        );
        assert_eq!(
            snapshot.sample.champion_aggregates.analyzed_matches,
            MAX_SEASON_MATCHES
        );
        assert!(snapshot.sample.champion_aggregates.truncated);
        assert_eq!(
            snapshot
                .champion_aggregates
                .iter()
                .map(|aggregate| aggregate.games)
                .sum::<u32>(),
            MAX_SEASON_MATCHES as u32
        );
    }

    fn riot_id() -> RiotIdInput {
        RiotIdInput {
            game_name: "Longcat".to_owned(),
            tag_line: "KR1".to_owned(),
            platform: "KR".to_owned(),
        }
    }

    struct SampleClient {
        season_match_count: usize,
        requested_match_count: AtomicUsize,
        recent_start: AtomicUsize,
        recent_all_queues: AtomicBool,
        season_page_requests: AtomicUsize,
        season_start: AtomicI64,
        season_ranked_only: AtomicBool,
        provider_fetches: AtomicUsize,
        generation: AtomicUsize,
        fail_provider: AtomicBool,
    }

    impl SampleClient {
        fn new(season_match_count: usize) -> Self {
            Self {
                season_match_count,
                requested_match_count: AtomicUsize::new(0),
                recent_start: AtomicUsize::new(0),
                recent_all_queues: AtomicBool::new(false),
                season_page_requests: AtomicUsize::new(0),
                season_start: AtomicI64::new(0),
                season_ranked_only: AtomicBool::new(false),
                provider_fetches: AtomicUsize::new(0),
                generation: AtomicUsize::new(0),
                fail_provider: AtomicBool::new(false),
            }
        }
    }

    impl RiotClient for SampleClient {
        fn account_by_riot_id<'a>(
            &'a self,
            _: RoutingRegion,
            _: &'a str,
            _: &'a str,
        ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>> {
            Box::pin(async move {
                self.provider_fetches.fetch_add(1, Ordering::SeqCst);
                if self.fail_provider.load(Ordering::SeqCst) {
                    return Err(RiotClientError::Timeout);
                }
                self.generation.fetch_add(1, Ordering::SeqCst);
                Ok(account())
            })
        }

        fn account_by_puuid<'a>(
            &'a self,
            _: RoutingRegion,
            _: &'a str,
        ) -> BoxFuture<'a, Result<RiotAccountDto, RiotClientError>> {
            Box::pin(async { Ok(account()) })
        }

        fn summoner_by_puuid<'a>(
            &'a self,
            _: Platform,
            _: &'a str,
        ) -> BoxFuture<'a, Result<SummonerDto, RiotClientError>> {
            Box::pin(async {
                Ok(SummonerDto {
                    profile_icon_id: Some(6668),
                    summoner_level: Some(258),
                })
            })
        }

        fn league_entries_by_puuid<'a>(
            &'a self,
            _: Platform,
            _: &'a str,
        ) -> BoxFuture<'a, Result<Vec<LeagueEntryDto>, RiotClientError>> {
            Box::pin(async {
                Ok(vec![
                    LeagueEntryDto {
                        queue_type: "RANKED_FLEX_SR".to_owned(),
                        tier: Some("GOLD".to_owned()),
                        rank: Some("II".to_owned()),
                        league_points: 45,
                        wins: 5,
                        losses: 6,
                    },
                    LeagueEntryDto {
                        queue_type: "RANKED_SOLO_5x5".to_owned(),
                        tier: Some("PLATINUM".to_owned()),
                        rank: Some("III".to_owned()),
                        league_points: 23,
                        wins: 22,
                        losses: 29,
                    },
                ])
            })
        }

        fn match_ids<'a>(
            &'a self,
            _: RoutingRegion,
            _: &'a str,
            query: MatchIdsQuery,
        ) -> BoxFuture<'a, Result<Vec<String>, RiotClientError>> {
            Box::pin(async move {
                let generation = self.generation.load(Ordering::SeqCst);
                if let Some(season_start) = query.start_time {
                    self.season_page_requests.fetch_add(1, Ordering::SeqCst);
                    self.season_start.store(season_start, Ordering::SeqCst);
                    self.season_ranked_only.store(
                        query.match_type == Some(MatchType::Ranked),
                        Ordering::SeqCst,
                    );
                    let end = (query.start + query.count).min(self.season_match_count);
                    if query.start >= end {
                        return Ok(Vec::new());
                    }
                    return Ok((query.start..end)
                        .map(|index| format!("GEN{generation}-MATCH{index:03}"))
                        .collect());
                }

                self.requested_match_count
                    .store(query.count, Ordering::SeqCst);
                self.recent_start.store(query.start, Ordering::SeqCst);
                self.recent_all_queues.store(
                    query.start_time.is_none() && query.match_type.is_none(),
                    Ordering::SeqCst,
                );
                let end = (query.start + query.count + 3).min(self.season_match_count);
                Ok((query.start..end)
                    .map(|index| format!("GEN{generation}-MATCH{index:03}"))
                    .collect())
            })
        }

        fn match_detail<'a>(
            &'a self,
            _: RoutingRegion,
            match_id: &'a str,
        ) -> BoxFuture<'a, Result<MatchDto, RiotClientError>> {
            let match_id = match_id.to_owned();
            Box::pin(async move {
                let index = match_id
                    .rsplit_once("MATCH")
                    .and_then(|(_, value)| value.parse::<usize>().ok())
                    .expect("sample match ID");
                tokio::task::yield_now().await;
                Ok(match_detail(match_id, index))
            })
        }
    }

    fn account() -> RiotAccountDto {
        RiotAccountDto {
            puuid: "target-puuid".to_owned(),
            game_name: "Longcat".to_owned(),
            tag_line: "KR1".to_owned(),
        }
    }

    fn match_detail(match_id: String, index: usize) -> MatchDto {
        let target_won = match index {
            115..125 => false,
            125..135 => true,
            _ => index.is_multiple_of(2),
        };
        MatchDto {
            metadata: MatchMetadataDto { match_id },
            info: MatchInfoDto {
                game_creation: 1_720_000_000_000 + index as i64 * 60_000,
                game_duration: 1_800 + index as i64,
                game_version: "16.13.1".to_owned(),
                queue_id: 420,
                participants: vec![
                    participant("target-puuid", champion_for_index(index), 100, target_won),
                    participant("opponent-puuid", "Opponent", 200, !target_won),
                ],
                teams: vec![team(100, target_won, 11, 3), team(200, !target_won, 3, 2)],
            },
        }
    }

    fn champion_for_index(index: usize) -> &'static str {
        match index {
            0..40 => "Ahri",
            40..70 => "LeeSin",
            70..95 => "Gnar",
            95..115 => "Lux",
            115..125 => "Vi",
            _ => "Zed",
        }
    }

    fn participant(
        puuid: &str,
        champion_name: &str,
        team_id: i32,
        win: bool,
    ) -> MatchParticipantDto {
        MatchParticipantDto {
            puuid: puuid.to_owned(),
            riot_id_game_name: Some("Player".to_owned()),
            riot_id_tagline: Some("KR1".to_owned()),
            champion_name: champion_name.to_owned(),
            champ_level: 18,
            team_id,
            win,
            kills: 10,
            deaths: 2,
            assists: 8,
            total_minions_killed: 200,
            neutral_minions_killed: 20,
            gold_earned: 14_000,
            total_damage_dealt_to_champions: 30_000,
            total_damage_taken: 20_000,
            vision_score: 40,
            summoner1_id: 4,
            summoner2_id: 11,
            item0: 1001,
            item1: 1002,
            item2: 1003,
            item3: 1004,
            item4: 1005,
            item5: 1006,
            item6: 3364,
            triple_kills: 1,
            quadra_kills: 1,
            penta_kills: 1,
            team_position: "JUNGLE".to_owned(),
        }
    }

    fn team(team_id: i32, win: bool, tower_kills: i32, dragon_kills: i32) -> MatchTeamDto {
        MatchTeamDto {
            team_id,
            win,
            objectives: MatchObjectivesDto {
                baron: objective(1),
                champion: objective(if team_id == 100 { 40 } else { 25 }),
                dragon: objective(dragon_kills),
                horde: objective(3),
                inhibitor: objective(2),
                rift_herald: objective(1),
                tower: objective(tower_kills),
            },
        }
    }

    fn objective(kills: i32) -> MatchObjectiveDto {
        MatchObjectiveDto { kills }
    }
}
