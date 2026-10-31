"""Complete guild data-transfer export service."""

import json
import logging
import uuid
from datetime import UTC, datetime
from typing import Any

from pydantic import ValidationError

from monkeybot_api.core.errors import ApiError
from monkeybot_api.core.text import trim
from monkeybot_api.core.time import timestamp
from monkeybot_api.db.database import Database
from monkeybot_api.db.transactions import repeatable_read
from monkeybot_api.repositories.guilds import full_data_transfer as repository
from monkeybot_api.repositories.permissions import authorize
from monkeybot_api.schemas.guilds.full_data_transfer import FullDataExportData, FullSnapshotArchive

logger = logging.getLogger(__name__)

ARCHIVE_FORMAT = "MONKEYBOT_DATA_TRANSFER"
ARCHIVE_KIND = "GUILD_FULL_SNAPSHOT"
ARCHIVE_SCHEMA_VERSION = 1
MAX_TEXT_CHARS = 100
MAX_ARCHIVE_ROWS = 250_000
MAX_CHAMPION_ID_BYTES = 64
CHAMPION_ID_TABLE_NAMES = {
    "user_champion_preferences",
    "game_input_picks",
    "game_input_bans",
    "game_participants",
    "game_bans",
}


def valid_text(value: str) -> bool:
    return 1 <= len(trim(value)) <= MAX_TEXT_CHARS


def validate_archive(archive: FullSnapshotArchive) -> None:
    def invalid_archive() -> ApiError:
        return ApiError(
            400,
            "invalid_full_archive",
            "지원되는 MonkeyBot 전체 데이터 백업 파일 형식이 아닙니다.",
            {"kind": ARCHIVE_KIND, "schemaVersion": ARCHIVE_SCHEMA_VERSION},
        )

    if (
        archive.format != ARCHIVE_FORMAT
        or archive.kind != ARCHIVE_KIND
        or archive.schemaVersion != ARCHIVE_SCHEMA_VERSION
        or not valid_text(archive.source.guildName)
        or set(archive.tables) != set(repository.ARCHIVE_TABLE_NAMES)
    ):
        raise invalid_archive()
    if sum(len(rows) for rows in archive.tables.values()) > MAX_ARCHIVE_ROWS:
        raise ApiError(
            422,
            "full_archive_row_limit_exceeded",
            "전체 백업의 행 수가 지원 한도인 250,000개를 초과했습니다.",
            {"maxRows": MAX_ARCHIVE_ROWS},
        )
    guild_scoped_tables = {
        *repository.GUILD_SCOPED_TABLE_NAMES,
        "mmr_ledger_entries",
        "admin_audit_log",
    }
    # Rust's BTreeMap visits tables in lexical order, including on the error path.
    for table, rows in sorted(archive.tables.items()):
        for row in rows:
            if table in guild_scoped_tables and row.get("guild_id") != archive.source.guildId:
                raise invalid_archive()
            if (
                table == "user_guild_preferences"
                and row.get("default_guild_id") != archive.source.guildId
            ):
                raise invalid_archive()
            if table not in CHAMPION_ID_TABLE_NAMES:
                continue
            champion_id = row.get("champion_id")
            if "champion_id" in row and (
                (champion_id is None and table == "game_participants")
                or (
                    isinstance(champion_id, str)
                    and 1 <= len(champion_id) <= MAX_CHAMPION_ID_BYTES
                    and champion_id.isascii()
                    and champion_id.isalnum()
                )
            ):
                continue
            raise ApiError(
                422,
                "full_archive_constraint_violation",
                "전체 백업의 챔피언 ID에 일반 게임에서 사용할 수 없는 값이 포함되어 있습니다.",
                {"table": table},
            )


def summary(tables: dict[str, list[dict[str, Any]]]) -> dict[str, int]:
    def count(name: str) -> int:
        return len(tables.get(name, []))

    return {
        "totalRows": sum(len(rows) for rows in tables.values()),
        "users": count("users"),
        "members": count("guild_members"),
        "seasons": count("seasons"),
        "recruitments": count("recruitments") + count("auction_recruitments"),
        "series": count("series"),
        "games": count("games"),
        "auctionTournaments": count("auction_tournaments"),
        "auctionMatches": count("auction_matches"),
        "mmrLedgerEntries": count("mmr_ledger_entries"),
        "auditEntries": count("admin_audit_log"),
    }


def validate_export(value: dict[str, Any], request_id: str) -> dict[str, Any]:
    try:
        data = FullDataExportData.model_validate(value)
    except ValidationError:
        logger.exception("full_data_export_projection_failed", extra={"request_id": request_id})
        raise ApiError.from_status(500) from None
    validate_archive(data.archive)
    return data.model_dump()


async def export_full_data(
    database: Database, guild: str, actor: str, request_id: str
) -> dict[str, Any]:
    now = datetime.now(UTC)
    async with database.connection(transaction=True) as c:
        await repeatable_read(c)
        try:
            await authorize(c, guild, actor)
        except ApiError as exc:
            if exc.code == "not_found":
                raise ApiError(404, "not_found", "요청한 길드를 찾을 수 없습니다.") from None
            raise
        guild_name = await repository.guild_display_name(c, guild)
        if guild_name is None or not valid_text(guild_name):
            logger.error("full_data_export_guild_name_invalid", extra={"request_id": request_id})
            raise ApiError.from_status(500)
        tables = await repository.export_tables(c, guild)
        archive = {
            "format": ARCHIVE_FORMAT,
            "kind": ARCHIVE_KIND,
            "schemaVersion": ARCHIVE_SCHEMA_VERSION,
            "exportedAt": timestamp(now),
            "source": {"guildId": guild, "guildName": guild_name},
            "tables": tables,
        }
        export_summary = summary(tables)
        data = validate_export(
            {
                "archive": archive,
                "fileName": f"monkeybot-full-{guild}-v{ARCHIVE_SCHEMA_VERSION}.json",
                "summary": export_summary,
            },
            request_id,
        )
        await repository.insert_audit(
            c,
            {
                "guild": guild,
                "audit_id": uuid.uuid7(),
                "actor_id": actor,
                "request_id": request_id,
                "before": json.dumps({"schemaVersion": ARCHIVE_SCHEMA_VERSION}),
                "after": json.dumps(export_summary),
                "now": now,
            },
        )
        return data
