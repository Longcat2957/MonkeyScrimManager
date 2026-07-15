use sqlx::FromRow;
use thiserror::Error;
use time::OffsetDateTime;

use crate::{
    domain::{GuildId, Lane, SeasonId, UserId},
    infrastructure::postgres::DatabaseTransaction,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MmrLaneKey {
    pub user_id: UserId,
    pub lane: Lane,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LockedMmrRow {
    pub key: MmrLaneKey,
    pub mmr: f64,
    pub games_played: i32,
    pub wins: i32,
    pub revision: i64,
}

pub async fn ensure_and_lock_mmr_rows(
    transaction: &mut DatabaseTransaction,
    guild_id: &GuildId,
    season_id: SeasonId,
    keys: impl IntoIterator<Item = MmrLaneKey>,
    now: OffsetDateTime,
) -> Result<Vec<LockedMmrRow>, MmrRepositoryError> {
    let mut keys = keys.into_iter().collect::<Vec<_>>();
    keys.sort_by(|left, right| {
        left.user_id
            .as_str()
            .cmp(right.user_id.as_str())
            .then_with(|| left.lane.as_str().cmp(right.lane.as_str()))
    });
    keys.dedup();

    if keys.is_empty() {
        return Ok(Vec::new());
    }

    let user_ids = keys
        .iter()
        .map(|key| key.user_id.as_str().to_owned())
        .collect::<Vec<_>>();
    let lanes = keys
        .iter()
        .map(|key| key.lane.as_str().to_owned())
        .collect::<Vec<_>>();

    sqlx::query(
        "INSERT INTO user_lane_mmr (
             guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
         )
         SELECT $1, $2, input.user_id, input.lane, 1500, 0, 0, 0, $3
         FROM UNNEST($4::text[], $5::text[]) AS input(user_id, lane)
         ON CONFLICT (guild_id, season_id, user_id, lane) DO NOTHING",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(now)
    .bind(&user_ids)
    .bind(&lanes)
    .execute(&mut **transaction)
    .await?;

    let rows = sqlx::query_as::<_, StoredMmrRow>(
        "SELECT user_id, lane, mmr, games_played, wins, revision
         FROM user_lane_mmr
         WHERE guild_id = $1
           AND season_id = $2
           AND (user_id, lane) IN (
               SELECT input.user_id, input.lane
               FROM UNNEST($3::text[], $4::text[]) AS input(user_id, lane)
           )
         ORDER BY user_id, lane
         FOR UPDATE",
    )
    .bind(guild_id)
    .bind(season_id)
    .bind(&user_ids)
    .bind(&lanes)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() != keys.len() {
        return Err(MmrRepositoryError::MissingInitializedRow);
    }

    Ok(rows
        .into_iter()
        .map(|row| LockedMmrRow {
            key: MmrLaneKey {
                user_id: row.user_id,
                lane: row.lane,
            },
            mmr: row.mmr,
            games_played: row.games_played,
            wins: row.wins,
            revision: row.revision,
        })
        .collect())
}

#[derive(FromRow)]
struct StoredMmrRow {
    user_id: UserId,
    lane: Lane,
    mmr: f64,
    games_played: i32,
    wins: i32,
    revision: i64,
}

#[derive(Debug, Error)]
pub enum MmrRepositoryError {
    #[error("initialized MMR row could not be loaded")]
    MissingInitializedRow,
    #[error("PostgreSQL MMR operation failed")]
    Sql(#[from] sqlx::Error),
}
