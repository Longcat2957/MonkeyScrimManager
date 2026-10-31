from typing import Any

from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.time import timestamp
from monkeybot_api.repositories.auctions import notifications as storage
from monkeybot_api.repositories.effects import notification
from monkeybot_api.repositories.series.snapshots import score
from monkeybot_api.schemas.auctions.games import error


async def result_notification(c: AsyncConnection, p: dict[str, Any], revision: int) -> None:
    channel = await storage.result_channel(c, p)
    if channel is None:
        return
    tournaments = await storage.completed_tournament(c, p)
    if not tournaments:
        raise ApiError.from_status(500)
    t = tournaments[0]
    matches = await storage.completed_matches(c, p)
    expected = ["FINAL"] if t["format"] == "TEN_PERSON" else ["SEMIFINAL_A", "SEMIFINAL_B", "FINAL"]
    if not t["ended_early"] and [m["match_key"] for m in matches] != expected:
        raise ApiError.from_status(500)
    rendered = []
    for m in matches:
        games = await storage.confirmed_games(c, {**p, "match": m["id"]})
        sc = score(games)
        if not m["ended_early"]:
            if m["winner_name"] is not None and m["winner_name"] == m["left_name"]:
                winning_score = sc["team1"]
            elif m["winner_name"] is not None and m["winner_name"] == m["right_name"]:
                winning_score = sc["team2"]
            else:
                raise ApiError.from_status(500)
            if winning_score != m["best_of"] // 2 + 1 or len(games) > m["best_of"]:
                raise ApiError.from_status(500)
        rendered.append(
            {
                "endedEarly": m["ended_early"],
                "matchKey": m["match_key"],
                "label": {"SEMIFINAL_A": "준결승 A", "SEMIFINAL_B": "준결승 B", "FINAL": "결승"}[
                    m["match_key"]
                ],
                "bestOf": m["best_of"],
                "leftTeamName": m["left_name"],
                "rightTeamName": m["right_name"],
                "leftScore": sc["team1"],
                "rightScore": sc["team2"],
                "winnerTeamName": m["winner_name"],
            }
        )
    champion = None
    if t["champion_team_id"] is not None and t["champion_name"] is not None:
        champion = {"teamId": str(t["champion_team_id"]), "teamName": t["champion_name"]}
    elif not (t["ended_early"] and t["champion_team_id"] is None and t["champion_name"] is None):
        raise ApiError.from_status(500)
    if t["ended_at"] is None:
        raise ApiError.from_status(500)
    if champion is not None:
        final = next((m for m in rendered if m["matchKey"] == "FINAL"), None)
        if final is None or final["winnerTeamName"] != champion["teamName"]:
            raise ApiError.from_status(500)
    await notification(
        c,
        p,
        "AUCTION_TOURNAMENT_RESULT_CREATE",
        channel,
        f"auction-tournament:{p['tournament']}:result:{revision}",
        {
            "schemaVersion": 1,
            "guildId": p["guild"],
            "tournamentId": str(p["tournament"]),
            "channelId": channel,
            "tournament": {
                "endedEarly": t["ended_early"],
                "displayNumber": t["display_number"],
                "title": t["title"],
                "format": t["format"],
                "semifinalBestOf": t["semifinal_best_of"],
                "finalBestOf": t["final_best_of"],
                "hardFearless": t["hard_fearless"],
            },
            "champion": champion,
            "matches": rendered,
            "completedAt": timestamp(t["ended_at"]),
        },
    )


async def reconcile(c: AsyncConnection, p: dict[str, Any]) -> None:
    cards = await storage.lock_result_notifications(c, {**p, "target": str(p["tournament"])})
    if any(r["active"] for r in cards):
        raise error("auction_tournament_result_delivery_in_progress")
    for r in cards:
        if r["delivered_at"] is not None:
            if r["discord_message_id"] is None:
                raise ApiError.from_status(500)
            await notification(
                c,
                p,
                "AUCTION_TOURNAMENT_RESULT_DELETE",
                r["channel_id"],
                f"auction-tournament:{p['tournament']}:result-delete:{r['id']}",
                {
                    "schemaVersion": 1,
                    "guildId": p["guild"],
                    "tournamentId": str(p["tournament"]),
                    "channelId": r["channel_id"],
                    "discordMessageId": r["discord_message_id"],
                },
            )
        elif r["failed_at"] is None and r["attempts"] == 0:
            await storage.cancel_pending_notification(c, {**p, "card": r["id"]})
        elif r["failure_code"] != "RESULT_REVERTED":
            raise error("auction_tournament_result_delivery_unresolved")
