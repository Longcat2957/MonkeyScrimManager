import pytest

from monkeybot_api.core.errors import ApiError
from monkeybot_api.repositories.guilds.full_data_transfer import ARCHIVE_TABLE_NAMES
from monkeybot_api.schemas.guilds.full_data_transfer import FullSnapshotArchive
from monkeybot_api.services.guilds.full_data_transfer import (
    MAX_ARCHIVE_ROWS,
    valid_text,
    validate_archive,
)
from tests.support.guilds.context import GUILD_A, GUILD_B

CHAMPION_TABLES = (
    "user_champion_preferences",
    "game_input_picks",
    "game_input_bans",
    "game_participants",
    "game_bans",
)


@pytest.fixture
def archive() -> FullSnapshotArchive:
    return FullSnapshotArchive.model_validate(
        {
            "format": "MONKEYBOT_DATA_TRANSFER",
            "kind": "GUILD_FULL_SNAPSHOT",
            "schemaVersion": 1,
            "exportedAt": "2026-10-03T00:00:00Z",
            "source": {"guildId": GUILD_A, "guildName": "Export guild"},
            "tables": {name: [] for name in ARCHIVE_TABLE_NAMES},
        }
    )


def assert_invalid_archive(error: ApiError) -> None:
    assert error.status == 400
    assert error.code == "invalid_full_archive"
    assert error.message == "지원되는 MonkeyBot 전체 데이터 백업 파일 형식이 아닙니다."
    assert error.details == {"kind": "GUILD_FULL_SNAPSHOT", "schemaVersion": 1}


@pytest.mark.parametrize("change", ["missing", "extra"])
def test_archive_requires_exact_portable_table_set(archive, change):
    validate_archive(archive)
    if change == "missing":
        del archive.tables["games"]
    else:
        archive.tables["user_sessions"] = []
    with pytest.raises(ApiError) as caught:
        validate_archive(archive)
    assert_invalid_archive(caught.value)


@pytest.mark.parametrize(
    ("table", "column"),
    [
        ("seasons", "guild_id"),
        ("mmr_ledger_entries", "guild_id"),
        ("admin_audit_log", "guild_id"),
        ("user_guild_preferences", "default_guild_id"),
    ],
)
def test_archive_rejects_cross_guild_rows(archive, table, column):
    archive.tables[table] = [{column: GUILD_B}]
    with pytest.raises(ApiError) as caught:
        validate_archive(archive)
    assert_invalid_archive(caught.value)


def test_archive_enforces_total_row_limit_including_global_rows(archive):
    archive.tables["users"] = [{}] * MAX_ARCHIVE_ROWS
    validate_archive(archive)
    archive.tables["seasons"] = [{"guild_id": GUILD_A}]
    with pytest.raises(ApiError) as caught:
        validate_archive(archive)
    error = caught.value
    assert error.status == 422
    assert error.code == "full_archive_row_limit_exceeded"
    assert error.message == "전체 백업의 행 수가 지원 한도인 250,000개를 초과했습니다."
    assert error.details == {"maxRows": 250_000}


@pytest.mark.parametrize("table", CHAMPION_TABLES)
def test_archive_rejects_mode_specific_champion_ids(archive, table):
    archive.tables[table] = [{"guild_id": GUILD_A, "champion_id": "Jade_Ahri"}]
    with pytest.raises(ApiError) as caught:
        validate_archive(archive)
    error = caught.value
    assert error.status == 422
    assert error.code == "full_archive_constraint_violation"
    assert error.message == (
        "전체 백업의 챔피언 ID에 일반 게임에서 사용할 수 없는 값이 포함되어 있습니다."
    )
    assert error.details == {"table": table}


@pytest.mark.parametrize("champion_id", ["", "A" * 65, "아리", None, 1])
def test_archive_rejects_invalid_champion_id_values(archive, champion_id):
    archive.tables["user_champion_preferences"] = [{"champion_id": champion_id}]
    with pytest.raises(ApiError) as caught:
        validate_archive(archive)
    assert caught.value.code == "full_archive_constraint_violation"


@pytest.mark.parametrize("table", CHAMPION_TABLES)
def test_archive_requires_champion_field_even_when_null_is_allowed(archive, table):
    archive.tables[table] = [{"guild_id": GUILD_A}]
    with pytest.raises(ApiError) as caught:
        validate_archive(archive)
    assert caught.value.code == "full_archive_constraint_violation"


def test_archive_accepts_canonical_champions_and_unselected_game_participant(archive):
    for table in CHAMPION_TABLES:
        archive.tables[table] = [
            {"guild_id": GUILD_A, "champion_id": champion}
            for champion in ("Ahri", "KSante", "A" * 64)
        ]
    archive.tables["game_participants"].append({"guild_id": GUILD_A, "champion_id": None})
    validate_archive(archive)


@pytest.mark.parametrize("name", ["", " \t\n", "가" * 101])
def test_archive_rejects_invalid_source_name(archive, name):
    archive.source.guildName = name
    with pytest.raises(ApiError) as caught:
        validate_archive(archive)
    assert_invalid_archive(caught.value)


def test_guild_name_length_and_trimming_follow_rust_unicode_rules():
    assert valid_text("\u2003" + "가" * 100 + "\u2003")
    assert valid_text("\x1c")
    assert not valid_text("\x1c" + "가" * 100)
