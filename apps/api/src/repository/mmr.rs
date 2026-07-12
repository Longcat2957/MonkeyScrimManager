use sqlx::Row;
use thiserror::Error;
use time::OffsetDateTime;

use crate::{
    domain::{DiscordUserId, GuildId, Lane, SeasonId},
    infrastructure::postgres::DatabaseTransaction,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MmrLaneKey {
    pub user_id: DiscordUserId,
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

    for key in &keys {
        sqlx::query(
            "INSERT INTO user_lane_mmr (
                guild_id, season_id, user_id, lane, mmr, games_played, wins, revision, updated_at
             ) VALUES ($1, $2, $3, $4, 1500, 0, 0, 0, $5)
             ON CONFLICT (guild_id, season_id, user_id, lane) DO NOTHING",
        )
        .bind(guild_id)
        .bind(season_id)
        .bind(&key.user_id)
        .bind(key.lane)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
    }

    let mut locked = Vec::with_capacity(keys.len());
    for key in keys {
        let row = sqlx::query(
            "SELECT mmr, games_played, wins, revision
             FROM user_lane_mmr
             WHERE guild_id = $1 AND season_id = $2 AND user_id = $3 AND lane = $4
             FOR UPDATE",
        )
        .bind(guild_id)
        .bind(season_id)
        .bind(&key.user_id)
        .bind(key.lane)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(MmrRepositoryError::MissingInitializedRow)?;

        locked.push(LockedMmrRow {
            key,
            mmr: row.try_get("mmr")?,
            games_played: row.try_get("games_played")?,
            wins: row.try_get("wins")?,
            revision: row.try_get("revision")?,
        });
    }

    Ok(locked)
}

#[derive(Debug, Error)]
pub enum MmrRepositoryError {
    #[error("initialized MMR row could not be loaded")]
    MissingInitializedRow,
    #[error("PostgreSQL MMR operation failed")]
    Sql(#[from] sqlx::Error),
}
