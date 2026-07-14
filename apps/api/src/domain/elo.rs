use super::{Lane, UserId};

pub const DEFAULT_MMR: f64 = 1500.0;
pub const K_FACTOR: f64 = 32.0;

/// A series roster team, distinct from the in-game BLUE/RED side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RosterTeam {
    Team1,
    Team2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchupPlayer {
    pub user_id: UserId,
    pub mmr: f64,
}

/// One fixed-lane TEAM_1 versus TEAM_2 matchup from a normal scrim roster.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneMatchup {
    pub lane: Lane,
    pub team_1: MatchupPlayer,
    pub team_2: MatchupPlayer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EloUpdate {
    pub user_id: UserId,
    pub opponent_user_id: UserId,
    pub mmr_before: f64,
    pub mmr_after: f64,
    pub delta: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaneMatchupResult {
    pub lane: Lane,
    pub team_1: EloUpdate,
    pub team_2: EloUpdate,
}

/// Returns the probability that `my_mmr` beats `opponent_mmr`.
pub fn expected_score(my_mmr: f64, opponent_mmr: f64) -> f64 {
    1.0 / (1.0 + 10.0_f64.powf((opponent_mmr - my_mmr) / 400.0))
}

/// Calculates one player's ELO update with MonkeyBot's fixed K factor.
pub fn update_elo(my_mmr: f64, opponent_mmr: f64, won: bool) -> (f64, f64) {
    let actual = if won { 1.0 } else { 0.0 };
    let delta = K_FACTOR * (actual - expected_score(my_mmr, opponent_mmr));
    (my_mmr + delta, delta)
}

/// Applies the v3 lane-by-lane ELO calculation to a normal scrim game.
///
/// The caller supplies only valid frozen-roster same-lane matchups. This function intentionally
/// performs no persistence, rounding, roster validation, or side-to-roster-team conversion.
pub fn apply_game_elo(
    matchups: impl IntoIterator<Item = LaneMatchup>,
    winner: RosterTeam,
) -> Vec<LaneMatchupResult> {
    let team_1_won = winner == RosterTeam::Team1;

    matchups
        .into_iter()
        .map(|matchup| {
            let team_1_user_id = matchup.team_1.user_id;
            let team_2_user_id = matchup.team_2.user_id;
            let (team_1_after, team_1_delta) =
                update_elo(matchup.team_1.mmr, matchup.team_2.mmr, team_1_won);
            let (team_2_after, team_2_delta) =
                update_elo(matchup.team_2.mmr, matchup.team_1.mmr, !team_1_won);

            LaneMatchupResult {
                lane: matchup.lane,
                team_1: EloUpdate {
                    user_id: team_1_user_id.clone(),
                    opponent_user_id: team_2_user_id.clone(),
                    mmr_before: matchup.team_1.mmr,
                    mmr_after: team_1_after,
                    delta: team_1_delta,
                },
                team_2: EloUpdate {
                    user_id: team_2_user_id,
                    opponent_user_id: team_1_user_id,
                    mmr_before: matchup.team_2.mmr,
                    mmr_after: team_2_after,
                    delta: team_2_delta,
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-10,
            "expected {expected}, got {actual}"
        );
    }

    fn player(id: &str, mmr: f64) -> MatchupPlayer {
        MatchupPlayer {
            user_id: UserId::parse(id).expect("test user ID"),
            mmr,
        }
    }

    #[test]
    fn expected_score_matches_v3_golden_values() {
        assert_close(expected_score(1500.0, 1500.0), 0.5);
        assert_close(expected_score(1900.0, 1500.0), 0.909_090_909_090_909_1);
        assert_close(expected_score(1100.0, 1500.0), 0.090_909_090_909_090_91);
        assert_close(
            expected_score(1500.0, 1700.0) + expected_score(1700.0, 1500.0),
            1.0,
        );
    }

    #[test]
    fn updates_equal_mmr_without_rounding() {
        let (winner_after, winner_delta) = update_elo(DEFAULT_MMR, DEFAULT_MMR, true);
        let (loser_after, loser_delta) = update_elo(DEFAULT_MMR, DEFAULT_MMR, false);

        assert_close(winner_delta, K_FACTOR / 2.0);
        assert_close(winner_after, DEFAULT_MMR + K_FACTOR / 2.0);
        assert_close(loser_delta, -K_FACTOR / 2.0);
        assert_close(loser_after, DEFAULT_MMR - K_FACTOR / 2.0);
    }

    #[test]
    fn applies_each_same_lane_matchup_and_preserves_zero_sum() {
        let results = apply_game_elo(
            [
                LaneMatchup {
                    lane: Lane::Top,
                    team_1: player("100000000000000001", DEFAULT_MMR),
                    team_2: player("100000000000000002", DEFAULT_MMR),
                },
                LaneMatchup {
                    lane: Lane::Mid,
                    team_1: player("100000000000000003", 1800.0),
                    team_2: player("100000000000000004", 1200.0),
                },
            ],
            RosterTeam::Team1,
        );

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].lane, Lane::Top);
        assert_eq!(results[0].team_1.user_id.as_str(), "100000000000000001");
        assert_eq!(
            results[0].team_1.opponent_user_id.as_str(),
            "100000000000000002"
        );
        assert_close(results[0].team_1.delta, K_FACTOR / 2.0);
        assert_close(results[0].team_2.delta, -K_FACTOR / 2.0);
        assert!(results[1].team_1.delta > 0.0);
        assert!(results[1].team_1.delta < K_FACTOR * 0.2);

        let total_delta = results
            .iter()
            .map(|result| result.team_1.delta + result.team_2.delta)
            .sum::<f64>();
        assert_close(total_delta, 0.0);
    }

    #[test]
    fn team_2_win_uses_the_inverse_outcome_for_every_lane() {
        let results = apply_game_elo(
            [LaneMatchup {
                lane: Lane::Jungle,
                team_1: player("100000000000000001", DEFAULT_MMR),
                team_2: player("100000000000000002", DEFAULT_MMR),
            }],
            RosterTeam::Team2,
        );

        assert_close(results[0].team_1.delta, -K_FACTOR / 2.0);
        assert_close(results[0].team_2.delta, K_FACTOR / 2.0);
    }
}
