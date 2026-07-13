use std::collections::HashSet;

use sqlx::{FromRow, PgPool, Postgres, Transaction};
use thiserror::Error;

use crate::domain::{DiscordUserId, Lane};

pub const MAX_CHAMPIONS_PER_LANE: usize = 10;
const MAX_CHAMPION_ID_BYTES: usize = 64;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChampionPreferencesSnapshot {
    pub top: Vec<String>,
    pub jungle: Vec<String>,
    pub mid: Vec<String>,
    pub bottom: Vec<String>,
    pub support: Vec<String>,
}

pub async fn get(
    pool: &PgPool,
    user_id: &DiscordUserId,
) -> Result<ChampionPreferencesSnapshot, ChampionPreferencesRepositoryError> {
    let rows = fetch_rows(pool, user_id).await?;
    snapshot_from_rows(rows)
}

pub async fn replace_lane(
    pool: &PgPool,
    user_id: &DiscordUserId,
    lane: Lane,
    champion_ids: Vec<String>,
) -> Result<ChampionPreferencesSnapshot, ChampionPreferencesRepositoryError> {
    validate_champion_ids(&champion_ids)?;

    let mut transaction = pool.begin().await?;
    lock_user(&mut transaction, user_id).await?;

    sqlx::query(
        "DELETE FROM user_champion_preferences
         WHERE user_id = $1 AND lane = $2",
    )
    .bind(user_id)
    .bind(lane)
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "INSERT INTO user_champion_preferences (user_id, lane, position, champion_id)
         SELECT $1, $2, requested.position::smallint, requested.champion_id
         FROM unnest($3::text[]) WITH ORDINALITY
              AS requested(champion_id, position)",
    )
    .bind(user_id)
    .bind(lane)
    .bind(&champion_ids)
    .execute(&mut *transaction)
    .await?;

    let rows = fetch_rows(&mut *transaction, user_id).await?;
    let snapshot = snapshot_from_rows(rows)?;
    transaction.commit().await?;
    Ok(snapshot)
}

fn validate_champion_ids(
    champion_ids: &[String],
) -> Result<(), ChampionPreferencesRepositoryError> {
    if champion_ids.len() > MAX_CHAMPIONS_PER_LANE {
        return Err(ChampionPreferencesRepositoryError::TooManyChampions);
    }

    let mut seen = HashSet::with_capacity(champion_ids.len());
    for champion_id in champion_ids {
        if champion_id.is_empty()
            || champion_id.len() > MAX_CHAMPION_ID_BYTES
            || !champion_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(ChampionPreferencesRepositoryError::InvalidChampionId);
        }
        if !seen.insert(champion_id.as_str()) {
            return Err(ChampionPreferencesRepositoryError::DuplicateChampionId);
        }
    }
    Ok(())
}

async fn lock_user(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: &DiscordUserId,
) -> Result<(), ChampionPreferencesRepositoryError> {
    let exists = sqlx::query_scalar::<_, String>(
        "SELECT discord_id
         FROM users
         WHERE discord_id = $1 AND deleted_at IS NULL
         FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **transaction)
    .await?;

    if exists.is_none() {
        return Err(ChampionPreferencesRepositoryError::UserNotFound);
    }
    Ok(())
}

async fn fetch_rows<'e, E>(
    executor: E,
    user_id: &DiscordUserId,
) -> Result<Vec<ChampionPreferenceRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query_as::<_, ChampionPreferenceRow>(
        "SELECT lane, position, champion_id
         FROM user_champion_preferences
         WHERE user_id = $1
         ORDER BY CASE lane
                    WHEN 'TOP' THEN 1
                    WHEN 'JUNGLE' THEN 2
                    WHEN 'MID' THEN 3
                    WHEN 'BOTTOM' THEN 4
                    WHEN 'SUPPORT' THEN 5
                  END,
                  position",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
}

fn snapshot_from_rows(
    rows: Vec<ChampionPreferenceRow>,
) -> Result<ChampionPreferencesSnapshot, ChampionPreferencesRepositoryError> {
    let mut snapshot = ChampionPreferencesSnapshot::default();
    let mut previous: Option<(Lane, i16)> = None;

    for row in rows {
        let expected_position = match previous {
            Some((previous_lane, previous_position)) if previous_lane == row.lane => {
                previous_position + 1
            }
            _ => 1,
        };
        if row.position != expected_position {
            return Err(ChampionPreferencesRepositoryError::StoredInvariantViolation);
        }

        lane_values_mut(&mut snapshot, row.lane).push(row.champion_id);
        previous = Some((row.lane, row.position));
    }

    Ok(snapshot)
}

fn lane_values_mut(snapshot: &mut ChampionPreferencesSnapshot, lane: Lane) -> &mut Vec<String> {
    match lane {
        Lane::Top => &mut snapshot.top,
        Lane::Jungle => &mut snapshot.jungle,
        Lane::Mid => &mut snapshot.mid,
        Lane::Bottom => &mut snapshot.bottom,
        Lane::Support => &mut snapshot.support,
    }
}

#[derive(Debug, FromRow)]
struct ChampionPreferenceRow {
    lane: Lane,
    position: i16,
    champion_id: String,
}

#[derive(Debug, Error)]
pub enum ChampionPreferencesRepositoryError {
    #[error("PostgreSQL champion preferences repository operation failed")]
    Sql(#[from] sqlx::Error),
    #[error("user does not exist")]
    UserNotFound,
    #[error("a lane cannot have more than {MAX_CHAMPIONS_PER_LANE} champion preferences")]
    TooManyChampions,
    #[error("champion IDs must be non-empty ASCII alphanumeric identifiers")]
    InvalidChampionId,
    #[error("champion IDs must be unique within a lane")]
    DuplicateChampionId,
    #[error("stored champion preference positions are not contiguous")]
    StoredInvariantViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn validator_accepts_empty_and_ordered_unique_champion_ids() {
        assert!(validate_champion_ids(&[]).is_ok());
        assert!(validate_champion_ids(&ids(&["LeeSin", "MonkeyKing", "KSante"])).is_ok());
    }

    #[test]
    fn validator_rejects_more_than_ten_champions() {
        let champion_ids = (0..=MAX_CHAMPIONS_PER_LANE)
            .map(|index| format!("Champion{index}"))
            .collect::<Vec<_>>();

        assert!(matches!(
            validate_champion_ids(&champion_ids),
            Err(ChampionPreferencesRepositoryError::TooManyChampions)
        ));
    }

    #[test]
    fn validator_rejects_duplicates_without_silently_deduplicating() {
        assert!(matches!(
            validate_champion_ids(&ids(&["Ahri", "Orianna", "Ahri"])),
            Err(ChampionPreferencesRepositoryError::DuplicateChampionId)
        ));
    }

    #[test]
    fn validator_rejects_non_canonical_champion_ids() {
        for champion_ids in [ids(&[""]), ids(&["Lee Sin"]), ids(&["아리"])] {
            assert!(matches!(
                validate_champion_ids(&champion_ids),
                Err(ChampionPreferencesRepositoryError::InvalidChampionId)
            ));
        }
    }

    #[test]
    fn snapshot_requires_contiguous_positions_and_preserves_order() {
        let snapshot = snapshot_from_rows(vec![
            ChampionPreferenceRow {
                lane: Lane::Mid,
                position: 1,
                champion_id: "Ahri".to_owned(),
            },
            ChampionPreferenceRow {
                lane: Lane::Mid,
                position: 2,
                champion_id: "Orianna".to_owned(),
            },
        ])
        .expect("contiguous snapshot");
        assert_eq!(snapshot.mid, ids(&["Ahri", "Orianna"]));

        assert!(matches!(
            snapshot_from_rows(vec![ChampionPreferenceRow {
                lane: Lane::Top,
                position: 2,
                champion_id: "Gnar".to_owned(),
            }]),
            Err(ChampionPreferencesRepositoryError::StoredInvariantViolation)
        ));
    }
}
