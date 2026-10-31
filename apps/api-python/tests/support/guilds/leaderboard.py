"""Small leaderboard fixtures with played/unplayed members and game parent variants."""

import uuid

import httpx
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection, AsyncEngine

from tests.support.guilds.context import (
    GUILD_A,
    GUILD_B,
    ROLE_B,
    USER_ID,
    authenticated_setup,
    guild_fixture,
)

PLAYER = "100000000000000003"
ZERO_PLAYER = "100000000000000004"
INACTIVE_PLAYER = "100000000000000005"
DUMMY = "dmy_01900000-0000-7000-8000-000000000006"
PATH = f"/api/guilds/{GUILD_A}/leaderboard"


async def setup(owner: AsyncEngine, client: httpx.AsyncClient) -> uuid.UUID:
    await authenticated_setup(owner, client)
    other_season = await guild_fixture(owner, GUILD_B, "Foreign guild", ROLE_B)
    async with owner.begin() as c:
        season = await c.scalar(
            text("SELECT id FROM seasons WHERE guild_id=:guild"), {"guild": GUILD_A}
        )
        assert isinstance(season, uuid.UUID)
        for user, name, status, lane in (
            (PLAYER, "Longcat", "ACTIVE", "BOTTOM"),
            (ZERO_PLAYER, "Unplayed", "ACTIVE", "MID"),
            (INACTIVE_PLAYER, "Inactive", "INACTIVE", "MID"),
            (DUMMY, "Dummy", "ACTIVE", "SUPPORT"),
        ):
            values = {"user": user, "name": name, "status": status, "lane": lane, "guild": GUILD_A}
            await c.execute(
                text("""
                    INSERT INTO users (user_id, identity_kind, display_name, created_at, updated_at)
                    VALUES (:user, :kind, :name, now(), now())
                """),
                {**values, "kind": "DUMMY" if user == DUMMY else "DISCORD"},
            )
            await c.execute(
                text("""
                    INSERT INTO guild_members
                        (guild_id, user_id, display_name, status, main_lane, first_seen_at)
                    VALUES (:guild, :user, :name, :status, :lane, now())
                """),
                values,
            )
        for user, lane, mmr, games, wins in (
            (PLAYER, "MID", 1600.0, 2, 1),
            (PLAYER, "BOTTOM", 1400.0, 1, 1),
            (PLAYER, "SUPPORT", 9999.0, 0, 0),
            (USER_ID, "TOP", 1510.0, 3, 2),
            (USER_ID, "JUNGLE", 1510.0, 3, 2),
            (ZERO_PLAYER, "MID", 2500.0, 0, 0),
            (INACTIVE_PLAYER, "MID", 3000.0, 10, 10),
            (DUMMY, "MID", 1600.0, 2, 1),
            (DUMMY, "BOTTOM", 1400.0, 1, 1),
        ):
            await c.execute(
                text("""
                    INSERT INTO user_lane_mmr
                        (guild_id, season_id, user_id, lane, mmr, games_played, wins,
                         revision, updated_at)
                    VALUES (:guild, :season, :user, :lane, :mmr, :games, :wins, 0, now())
                """),
                {
                    "guild": GUILD_A,
                    "season": season,
                    "user": user,
                    "lane": lane,
                    "mmr": mmr,
                    "games": games,
                    "wins": wins,
                },
            )
        await c.execute(
            text("""
                INSERT INTO user_lane_mmr
                    (guild_id, season_id, user_id, lane, mmr, games_played, wins,
                     revision, updated_at)
                VALUES (:guild, :season, :viewer, 'TOP', 9999, 20, 20, 0, now())
            """),
            {"guild": GUILD_B, "season": other_season, "viewer": USER_ID},
        )
        await c.execute(
            text("""
                INSERT INTO user_privacy_settings
                    (user_id, public_profile_enabled, show_riot_ids,
                     allow_external_record_search, updated_at)
                VALUES (:user, false, true, false, now())
            """),
            {"user": PLAYER},
        )
        await c.execute(
            text("UPDATE user_privacy_settings SET show_riot_ids=false WHERE user_id=:user"),
            {"user": USER_ID},
        )
        for user, name, icon, main in (
            (PLAYER, "Longcat", 7, True),
            (PLAYER, "Alternate", 8, False),
            (USER_ID, "PrivateViewer", 9, True),
        ):
            await c.execute(
                text("""
                    INSERT INTO riot_accounts
                        (puuid, user_id, platform, routing_region, game_name, tag_line,
                         normalized_game_name, normalized_tag_line, profile_icon_id,
                         is_main, last_synced_at, created_at, updated_at)
                    VALUES (:puuid, :user, 'KR', 'ASIA', :name, 'KR1', lower(:name),
                            'kr1', :icon, :main, now(), now(), now())
                """),
                {
                    "puuid": "leaderboard-" + name,
                    "user": user,
                    "name": name,
                    "icon": icon,
                    "main": main,
                },
            )
    return season


async def game(
    c: AsyncConnection,
    season: uuid.UUID,
    number: int,
    kind: str,
    champion: str | None,
) -> None:
    """One participant per game is sufficient to exercise champion parent filtering."""
    recruitment, parent, tournament, source_input, game_id = (uuid.uuid7() for _ in range(5))
    auction = kind.startswith(("auction", "match", "tournament"))
    values = {
        "guild": GUILD_A,
        "season": season,
        "recruitment": recruitment,
        "parent": parent,
        "tournament": tournament,
        "input": source_input,
        "game": game_id,
        "number": number,
        "viewer": USER_ID,
        "player": PLAYER,
        "champion": champion,
    }
    if auction:
        await c.execute(
            text("""
                INSERT INTO auction_recruitments
                    (guild_id, id, season_id, display_number, title, format, target_count,
                     status, revision, created_by, created_at, updated_at)
                VALUES (:guild, :recruitment, :season, :number, 'Auction', 'TEN_PERSON', 10,
                        'CONVERTED', 0, :viewer, now(), now())
            """),
            values,
        )
        await c.execute(
            text("""
                INSERT INTO auction_tournaments
                    (guild_id, id, season_id, source_recruitment_id, display_number, title,
                     format, status, revision, started_at, created_by, updated_at)
                VALUES (:guild, :tournament, :season, :recruitment, :number, 'Auction',
                        'TEN_PERSON', 'IN_GAME', 0, now(), :viewer, now())
            """),
            values,
        )
        await c.execute(
            text("""
                INSERT INTO auction_matches
                    (guild_id, tournament_id, id, season_id, match_key, best_of,
                     status, revision, updated_at)
                VALUES (:guild, :tournament, :parent, :season, 'FINAL', 1,
                        'LINEUP_PENDING', 0, now())
            """),
            values,
        )
    else:
        await c.execute(
            text("""
                INSERT INTO recruitments
                    (guild_id, id, season_id, display_number, title, target_count, status,
                     revision, created_by, created_at, updated_at)
                VALUES (:guild, :recruitment, :season, :number, 'Regular', 10,
                        'CONVERTED', 0, :viewer, now(), now())
            """),
            values,
        )
        await c.execute(
            text("""
                INSERT INTO series
                    (guild_id, id, season_id, source_recruitment_id, display_number, title,
                     best_of, status, revision, started_at, updated_at)
                VALUES (:guild, :parent, :season, :recruitment, :number, 'Regular', 1,
                        'IN_PROGRESS', 0, now(), now())
            """),
            values,
        )
    values.update({"series": None if auction else parent, "match": parent if auction else None})
    await c.execute(
        text("""
            INSERT INTO game_inputs
                (guild_id, id, series_id, auction_match_id, set_number, side_preset,
                 winning_side, status, updated_at)
            VALUES (:guild, :input, :series, :match, 1, 'DEFAULT', 'BLUE', 'SUBMITTED', now())
        """),
        values,
    )
    await c.execute(
        text("""
            INSERT INTO games
                (guild_id, id, season_id, series_id, auction_match_id, source_input_id,
                 game_number, side_preset, winning_side, status, mmr_status, played_at,
                 confirmed_by, confirmed_by_type, confirmed_at)
            VALUES (:guild, :game, :season, :series, :match, :input, 1, 'DEFAULT', 'BLUE',
                    'CONFIRMED', 'LEGACY_NOT_APPLIED', now(), :viewer, 'USER', now())
        """),
        values,
    )
    await c.execute(
        text("""
            INSERT INTO game_participants (guild_id, game_id, user_id, team, lane, champion_id)
            VALUES (:guild, :game, :player, 'TEAM_1', 'MID', :champion)
        """),
        values,
    )
    changes = {
        "series_cancelled": "UPDATE series SET status='CANCELLED', terminal_reason='CANCELLED',"
        " ended_at=now() WHERE id=:parent",
        "series_deleted": "UPDATE series SET deleted_at=now() WHERE id=:parent",
        "match_cancelled": "UPDATE auction_matches SET status='CANCELLED', ended_at=now()"
        " WHERE id=:parent",
        "match_deleted": "UPDATE auction_matches SET deleted_at=now() WHERE id=:parent",
        "tournament_cancelled": "UPDATE auction_tournaments SET status='CANCELLED', ended_at=now()"
        " WHERE id=:tournament",
        "tournament_deleted": "UPDATE auction_tournaments SET deleted_at=now()"
        " WHERE id=:tournament",
        "voided": "UPDATE games SET status='VOIDED', mmr_status='REVERSED', voided_by=:viewer,"
        " voided_by_type='USER', voided_at=now(), void_reason='Cancelled' WHERE id=:game",
    }
    if kind in changes:
        await c.execute(text(changes[kind]), values)
