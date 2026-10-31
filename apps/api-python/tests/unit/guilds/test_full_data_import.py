import uuid

from monkeybot_api.repositories.guilds.full_data_import import TargetState
from monkeybot_api.repositories.guilds.full_data_transfer import ARCHIVE_TABLE_NAMES
from monkeybot_api.services.guilds.full_data_import import archive_digest, map_archive, stable_bytes
from tests.support.guilds.context import GUILD_A, GUILD_B


def test_mapping_remaps_guild_active_season_discord_links_and_display_numbers():
    source_season = str(uuid.uuid4())
    target_season = uuid.uuid4()
    archive = {
        "format": "MONKEYBOT_DATA_TRANSFER",
        "kind": "GUILD_FULL_SNAPSHOT",
        "schemaVersion": 1,
        "exportedAt": "2026-10-03T00:00:00Z",
        "source": {"guildId": GUILD_A, "guildName": "Source"},
        "tables": {name: [] for name in ARCHIVE_TABLE_NAMES},
    }
    archive["tables"]["seasons"] = [{"guild_id": GUILD_A, "id": source_season, "status": "ACTIVE"}]
    archive["tables"]["recruitments"] = [
        {
            "guild_id": GUILD_A,
            "season_id": source_season,
            "display_number": 2,
            "title": "일반 내전 모집 #2",
            "channel_id": "400000000000000001",
            "message_id": "400000000000000002",
        }
    ]
    archive["tables"]["guild_counters"] = [
        {"guild_id": GUILD_A, "counter_key": "RECRUITMENT", "next_value": 3}
    ]
    state = TargetState(
        1,
        0,
        target_season,
        {"RECRUITMENT": 5, "SERIES": 0, "AUCTION_RECRUITMENT": 0, "AUCTION_TOURNAMENT": 0},
        [],
        [],
        [],
        [],
    )
    original_digest = archive_digest(archive)
    mapped, remapped, used_target_season, rebased = map_archive(archive, GUILD_B, state)
    assert (remapped, used_target_season, rebased) == (True, True, True)
    assert mapped["source"]["guildId"] == GUILD_B
    assert mapped["tables"]["seasons"] == []
    recruitment = mapped["tables"]["recruitments"][0]
    assert recruitment["guild_id"] == GUILD_B
    assert recruitment["season_id"] == str(target_season)
    assert recruitment["display_number"] == 7
    assert recruitment["title"] == "일반 내전 모집 #7"
    assert recruitment["channel_id"] is None
    assert recruitment["message_id"] is None
    assert mapped["tables"]["guild_counters"] == [
        {"guild_id": GUILD_B, "counter_key": "RECRUITMENT", "next_value": 8}
    ]
    assert archive["tables"]["seasons"][0]["id"] == source_season
    assert archive_digest(archive) == original_digest


def test_digest_json_float_notation_matches_rust_serde():
    assert stable_bytes({"mmr": 1e-7}) == b'{"mmr":1e-7}'
