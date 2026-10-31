import math

from monkeybot_api.services.guilds.leaderboard import build_players


def test_signed_zero_weighting_preserves_rust_total_order() -> None:
    rows = [
        {
            "user_id": user_id,
            "name": "Player",
            "tag": None,
            "profile_icon_id": None,
            "main_lane": None,
            "lane": "TOP",
            "mmr": mmr,
            "games": 1,
            "wins": 0,
            "champion_id": None,
        }
        for user_id, mmr in (("100000000000000001", -0.0), ("100000000000000002", 0.0))
    ]

    players = build_players(rows, "100000000000000001")

    assert [player.id for player in players] == ["100000000000000002", "100000000000000001"]
    assert [math.copysign(1.0, player.compositeMmr) for player in players] == [1.0, -1.0]
