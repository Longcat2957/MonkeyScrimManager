"""Durable Riot records snapshot, page cache, and refresh gate."""

import json
import uuid
from datetime import timedelta
from typing import Any

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncConnection

from monkeybot_api.core.errors import ApiError
from monkeybot_api.db.database import Database


def key(lookup: dict[str, str]) -> dict[str, str]:
    return {
        "platform": lookup["platform"],
        "name": lookup["normalized_name"],
        "tag": lookup["normalized_tag"],
    }


async def snapshot(c: AsyncConnection, lookup: dict[str, str]) -> dict[str, Any] | None:
    row = (
        (
            await c.execute(
                text("""
        SELECT snapshot.puuid, snapshot.platform, snapshot.routing_region,
               snapshot.game_name, snapshot.tag_line, snapshot.schema_version,
               snapshot.snapshot, snapshot.refreshed_at
        FROM riot_records_lookup_aliases AS alias
        JOIN riot_records_snapshots AS snapshot
          ON snapshot.platform=alias.snapshot_platform AND snapshot.puuid=alias.snapshot_puuid
        WHERE alias.platform=:platform AND alias.normalized_game_name=:name
          AND alias.normalized_tag_line=:tag
    """),
                key(lookup),
            )
        )
        .mappings()
        .one_or_none()
    )
    return dict(row) if row else None


async def recent_page(
    c: AsyncConnection, lookup: dict[str, str], offset: int, count: int
) -> dict[str, Any] | None:
    row = (
        (
            await c.execute(
                text("""
        SELECT snapshot.platform, snapshot.game_name, snapshot.tag_line,
               page.page_offset, page.requested_count, page.schema_version,
               page.page, page.fetched_at
        FROM riot_records_lookup_aliases AS alias
        JOIN riot_records_snapshots AS snapshot
          ON snapshot.platform=alias.snapshot_platform AND snapshot.puuid=alias.snapshot_puuid
        JOIN riot_records_recent_pages AS page
          ON page.platform=snapshot.platform AND page.puuid=snapshot.puuid
        WHERE alias.platform=:platform AND alias.normalized_game_name=:name
          AND alias.normalized_tag_line=:tag
          AND page.page_offset=:offset AND page.requested_count=:count
    """),
                {**key(lookup), "offset": offset, "count": count},
            )
        )
        .mappings()
        .one_or_none()
    )
    return dict(row) if row else None


async def acquire_refresh(database: Database, lookup: dict[str, str]) -> dict[str, Any]:
    params = key(lookup)
    async with database.connection(transaction=True) as c:
        await c.execute(
            text("""
            INSERT INTO riot_records_refresh_gates (
                platform, normalized_game_name, normalized_tag_line,
                lease_token, lease_acquired_at, lease_until, created_at, updated_at
            ) VALUES (
                :platform, :name, :tag, NULL, NULL, NULL,
                clock_timestamp(), clock_timestamp()
            )
            ON CONFLICT (platform, normalized_game_name, normalized_tag_line) DO NOTHING
        """),
            params,
        )
        lease_until = await c.scalar(
            text("""
            SELECT lease_until FROM riot_records_refresh_gates
            WHERE platform=:platform AND normalized_game_name=:name
              AND normalized_tag_line=:tag FOR UPDATE
        """),
            params,
        )
        cached = await snapshot(c, lookup)
        now = await c.scalar(text("SELECT clock_timestamp()"))
        if cached is not None:
            retry = cached["refreshed_at"] + timedelta(seconds=30)
            if retry > now:
                return {"state": "cooldown", "snapshot": cached, "retry_at": retry}
        if lease_until is not None and lease_until > now:
            return {"state": "in_progress", "snapshot": cached, "retry_at": lease_until}
        token = uuid.uuid7()
        times = (
            (
                await c.execute(
                    text("""
            UPDATE riot_records_refresh_gates
            SET lease_token=:token, lease_acquired_at=statement_timestamp(),
                lease_until=statement_timestamp() + INTERVAL '5 minutes',
                updated_at=clock_timestamp()
            WHERE platform=:platform AND normalized_game_name=:name
              AND normalized_tag_line=:tag
            RETURNING lease_acquired_at, lease_until
        """),
                    {**params, "token": token},
                )
            )
            .mappings()
            .one()
        )
        return {
            "state": "acquired",
            "snapshot": cached,
            "token": token,
            "acquired_at": times["lease_acquired_at"],
            "expires_at": times["lease_until"],
        }


async def abort_refresh(database: Database, lookup: dict[str, str], token: uuid.UUID) -> None:
    async with database.connection(transaction=True) as c:
        await c.execute(
            text("""
            UPDATE riot_records_refresh_gates
            SET lease_token=NULL, lease_acquired_at=NULL, lease_until=NULL,
                updated_at=clock_timestamp()
            WHERE platform=:platform AND normalized_game_name=:name
              AND normalized_tag_line=:tag AND lease_token=:token
        """),
            {**key(lookup), "token": token},
        )


async def complete_refresh(
    database: Database,
    lookup: dict[str, str],
    token: uuid.UUID,
    record: dict[str, Any],
    normalized_name: str,
    normalized_tag: str,
    routing_region: str,
) -> None:
    params = {**key(lookup), "token": token}
    account = record["account"]
    async with database.connection(transaction=True) as c:
        current = await c.scalar(
            text("""
            SELECT lease_token FROM riot_records_refresh_gates
            WHERE platform=:platform AND normalized_game_name=:name
              AND normalized_tag_line=:tag FOR UPDATE
        """),
            params,
        )
        if current != token:
            raise ApiError.from_status(500)
        await c.execute(
            text("""
            INSERT INTO riot_records_snapshots (
                platform, puuid, routing_region, game_name, tag_line,
                normalized_game_name, normalized_tag_line, schema_version, snapshot,
                refreshed_at, created_at, updated_at
            ) VALUES (
                :platform, :puuid, :region, :game_name, :tag_line,
                :canonical_name, :canonical_tag, 1, CAST(:snapshot AS jsonb),
                clock_timestamp(), clock_timestamp(), clock_timestamp()
            )
            ON CONFLICT (platform, puuid) DO UPDATE
            SET routing_region=EXCLUDED.routing_region,
                game_name=EXCLUDED.game_name, tag_line=EXCLUDED.tag_line,
                normalized_game_name=EXCLUDED.normalized_game_name,
                normalized_tag_line=EXCLUDED.normalized_tag_line,
                schema_version=EXCLUDED.schema_version, snapshot=EXCLUDED.snapshot,
                refreshed_at=EXCLUDED.refreshed_at, updated_at=EXCLUDED.updated_at
        """),
            {
                "platform": lookup["platform"],
                "puuid": account["puuid"],
                "region": routing_region,
                "game_name": account["gameName"],
                "tag_line": account["tagLine"],
                "canonical_name": normalized_name,
                "canonical_tag": normalized_tag,
                "snapshot": json.dumps(record, ensure_ascii=False),
            },
        )
        await c.execute(
            text("""
            DELETE FROM riot_records_recent_pages
            WHERE platform=:platform AND puuid=:puuid
        """),
            {"platform": lookup["platform"], "puuid": account["puuid"]},
        )
        for name, tag in (
            (lookup["normalized_name"], lookup["normalized_tag"]),
            (normalized_name, normalized_tag),
        ):
            await c.execute(
                text("""
                INSERT INTO riot_records_lookup_aliases (
                    platform, normalized_game_name, normalized_tag_line,
                    snapshot_platform, snapshot_puuid, resolved_at
                ) VALUES (
                    :platform, :name, :tag, :platform, :puuid, clock_timestamp()
                )
                ON CONFLICT (platform, normalized_game_name, normalized_tag_line) DO UPDATE
                SET snapshot_platform=EXCLUDED.snapshot_platform,
                    snapshot_puuid=EXCLUDED.snapshot_puuid,
                    resolved_at=EXCLUDED.resolved_at
            """),
                {
                    "platform": lookup["platform"],
                    "name": name,
                    "tag": tag,
                    "puuid": account["puuid"],
                },
            )
        changed = await c.execute(
            text("""
            UPDATE riot_records_refresh_gates
            SET lease_token=NULL, lease_acquired_at=NULL, lease_until=NULL,
                updated_at=clock_timestamp()
            WHERE platform=:platform AND normalized_game_name=:name
              AND normalized_tag_line=:tag AND lease_token=:token
        """),
            params,
        )
        if changed.rowcount != 1:
            raise ApiError.from_status(500)


async def store_page(
    database: Database,
    lookup: dict[str, str],
    offset: int,
    count: int,
    page: dict[str, Any],
) -> None:
    async with database.connection(transaction=True) as c:
        owner = (
            (
                await c.execute(
                    text("""
            SELECT snapshot.platform, snapshot.puuid
            FROM riot_records_lookup_aliases AS alias
            JOIN riot_records_snapshots AS snapshot
              ON snapshot.platform=alias.snapshot_platform AND snapshot.puuid=alias.snapshot_puuid
            WHERE alias.platform=:platform AND alias.normalized_game_name=:name
              AND alias.normalized_tag_line=:tag FOR UPDATE OF snapshot
        """),
                    key(lookup),
                )
            )
            .mappings()
            .one_or_none()
        )
        if owner is None:
            raise ApiError.from_status(500)
        await c.execute(
            text("""
            INSERT INTO riot_records_recent_pages (
                platform, puuid, page_offset, requested_count, schema_version,
                page, fetched_at, created_at, updated_at
            ) VALUES (
                :platform, :puuid, :offset, :count, 1,
                CAST(:page AS jsonb), clock_timestamp(), clock_timestamp(), clock_timestamp()
            )
            ON CONFLICT (platform, puuid, page_offset, requested_count) DO UPDATE
            SET schema_version=EXCLUDED.schema_version, page=EXCLUDED.page,
                fetched_at=EXCLUDED.fetched_at, updated_at=EXCLUDED.updated_at
        """),
            {
                "platform": owner["platform"],
                "puuid": owner["puuid"],
                "offset": offset,
                "count": count,
                "page": json.dumps(page, ensure_ascii=False),
            },
        )
