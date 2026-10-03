"""Fenced SQLx-to-Alembic baseline handover and catalog verification.

The SQLx ledger is a historical record, never synthesized on Alembic-only databases.
The migration role invokes this CLI; the API uses only validate_migration_state.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
from collections.abc import Mapping
from pathlib import Path
from typing import Any

from alembic import command
from alembic.config import Config
from sqlalchemy import text
from sqlalchemy.engine import Connection, make_url
from sqlalchemy.exc import DBAPIError
from sqlalchemy.ext.asyncio import create_async_engine

BASELINE_REVISION = "20261003_baseline"
SQLX_MANIFEST_SHA384 = (
    "894000becec56f4661e1e0d7272d2716079774058d2b0426fcad877404eb2a7271e4696443468eff09"
    "f417a3deee120d"
)
# Frozen from SQLx 0001..0026 on the same PostgreSQL major as the migration test image.
BASELINE_CATALOG_SHA384 = (
    "9fa120522e6f6a7b791a6bdab539860d1ee7605f49efcb1a91a49850a38462573f56784462d3d50"
    "7b6c47a4b9b85b5e6"
)
_DB_DIR = Path(__file__).resolve().parent
_MIGRATION_DIR = _DB_DIR / "alembic"
_MANIFEST = _DB_DIR / "sqlx-manifest.json"
_METADATA_TABLES = frozenset({"_sqlx_migrations", "alembic_version", "migration_provenance"})
_APPEND_ONLY_TABLES = frozenset(
    {
        "auction_actions",
        "auction_point_ledger",
        "mmr_ledger_entries",
        "admin_audit_log",
        "dummy_user_audit_log",
    }
)
_ADVISORY_LOCK_KEY = 0x4D4F4E4B4559424F  # Separate fixed namespace for migration ownership.


class MigrationValidationError(ValueError):
    """Database cannot be trusted as one of the approved migration histories."""


def _manifest_entries() -> list[dict[str, Any]]:
    raw = _MANIFEST.read_bytes()
    if hashlib.sha384(raw).hexdigest() != SQLX_MANIFEST_SHA384:
        raise MigrationValidationError("packaged SQLx manifest checksum changed")
    manifest = json.loads(raw)
    entries: list[dict[str, Any]] = manifest["migrations"]
    if (
        manifest.get("algorithm") != "sha384"
        or [entry["version"] for entry in entries] != list(range(1, 27))
        or len({entry["file"] for entry in entries}) != 26
    ):
        raise MigrationValidationError("SQLx manifest is not exact 0001..0026")
    return entries


def validate_sqlx_history(connection: Connection) -> None:
    """Check every SQLx version, success flag, and SHA-384 checksum exactly."""
    expected = {entry["version"]: bytes.fromhex(entry["sha384"]) for entry in _manifest_entries()}
    if not _table_exists(connection, "_sqlx_migrations"):
        raise MigrationValidationError("SQLx migration ledger is missing")
    rows = connection.execute(
        text("SELECT version, success, checksum FROM public._sqlx_migrations")
    ).mappings()
    observed: dict[int, bytes] = {}
    for row in rows:
        version = int(row["version"])
        if version in observed or not row["success"]:
            raise MigrationValidationError("SQLx migration ledger has duplicate or failed rows")
        observed[version] = bytes(row["checksum"])
    if observed != expected:
        raise MigrationValidationError("SQLx migration ledger differs from frozen 0001..0026")


def verify_bootstrap_bundle() -> None:
    """Do not execute a copied SQL script unless it matches the frozen SQLx manifest."""
    entries = _manifest_entries()
    available = {path.name for path in (_MIGRATION_DIR / "sql").glob("*.sql")}
    if available != {entry["file"] for entry in entries}:
        raise MigrationValidationError("Alembic bootstrap SQL bundle differs from SQLx manifest")
    for entry in entries:
        contents = (_MIGRATION_DIR / "sql" / entry["file"]).read_bytes()
        if hashlib.sha384(contents).hexdigest() != entry["sha384"]:
            raise MigrationValidationError(f"Alembic SQL bundle checksum mismatch: {entry['file']}")


def bootstrap_sql() -> list[str]:
    verify_bootstrap_bundle()
    return [(_MIGRATION_DIR / "sql" / entry["file"]).read_text() for entry in _manifest_entries()]


def _table_exists(connection: Connection, name: str) -> bool:
    return bool(
        connection.scalar(
            text("SELECT to_regclass(:qualified) IS NOT NULL"), {"qualified": f"public.{name}"}
        )
    )


_CATALOG_QUERIES: Mapping[str, str] = {
    "extensions": """
        SELECT e.extname AS name, e.extversion AS version, n.nspname AS schema
        FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace
        WHERE n.nspname = 'public' ORDER BY e.extname
    """,
    "relations": """
        SELECT c.relname AS name, c.relkind AS kind, c.relpersistence AS persistence,
               c.relrowsecurity AS row_security, c.relforcerowsecurity AS force_row_security,
               pg_get_partkeydef(c.oid) AS partition_key
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')
          AND c.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance')
        ORDER BY c.relname
    """,
    "columns": """
        SELECT c.relname AS relation, a.attnum AS number, a.attname AS name,
               format_type(a.atttypid, a.atttypmod) AS data_type,
               a.attnotnull AS not_null, a.attidentity AS identity,
               a.attgenerated AS generated, a.attcollation::regcollation::text AS collation,
               pg_get_expr(d.adbin, d.adrelid) AS default_expr
        FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
        WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p', 'v', 'm', 'f')
          AND c.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance')
          AND a.attnum > 0 AND NOT a.attisdropped
        ORDER BY c.relname, a.attnum
    """,
    "constraints": """
        SELECT c.conname AS name, r.relname AS relation, c.contype AS kind,
               pg_get_constraintdef(c.oid, true) AS definition,
               c.condeferrable AS deferrable, c.condeferred AS deferred,
               c.convalidated AS validated
        FROM pg_constraint c JOIN pg_class r ON r.oid = c.conrelid
             JOIN pg_namespace n ON n.oid = r.relnamespace
        WHERE n.nspname = 'public'
          AND r.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance')
        ORDER BY r.relname, c.conname
    """,
    "indexes": """
        SELECT t.relname AS relation, i.relname AS name, pg_get_indexdef(i.oid) AS definition,
               x.indisunique AS unique_index, x.indisprimary AS primary_index,
               x.indisvalid AS valid, x.indisready AS ready
        FROM pg_index x JOIN pg_class i ON i.oid = x.indexrelid
             JOIN pg_class t ON t.oid = x.indrelid
             JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE n.nspname = 'public'
          AND t.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance')
        ORDER BY t.relname, i.relname
    """,
    "functions": """
        SELECT p.proname AS name, pg_get_function_identity_arguments(p.oid) AS arguments,
               pg_get_functiondef(p.oid) AS definition
        FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname = 'public' ORDER BY p.proname, arguments
    """,
    "triggers": """
        SELECT c.relname AS relation, t.tgname AS name, pg_get_triggerdef(t.oid) AS definition,
               t.tgenabled AS enabled
        FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public' AND NOT t.tgisinternal
          AND c.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance')
        ORDER BY c.relname, t.tgname
    """,
    "types": """
        SELECT t.typname AS name, t.typtype AS kind,
               format_type(t.typbasetype, t.typtypmod) AS base_type
        FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE n.nspname = 'public' AND t.typtype IN ('d', 'e', 'r', 'm')
        ORDER BY t.typname
    """,
    "sequences": """
        SELECT c.relname AS name, s.seqtypid::regtype::text AS data_type,
               s.seqstart AS start_value, s.seqincrement AS increment_by,
               s.seqmin AS min_value, s.seqmax AS max_value, s.seqcache AS cache_size,
               s.seqcycle AS cycle
        FROM pg_sequence s JOIN pg_class c ON c.oid = s.seqrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public' ORDER BY c.relname
    """,
}


def catalog_snapshot(connection: Connection) -> dict[str, list[dict[str, Any]]]:
    """Canonical schema catalog, excluding only migration bookkeeping relations."""
    return {
        kind: [dict(row) for row in connection.execute(text(query)).mappings()]
        for kind, query in _CATALOG_QUERIES.items()
    }


def _digest(value: Any) -> str:
    payload = json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        default=lambda item: item.decode("ascii") if isinstance(item, bytes) else str(item),
    ).encode()
    return hashlib.sha384(payload).hexdigest()


def catalog_sha384(connection: Connection) -> str:
    return _digest(catalog_snapshot(connection))


def grant_snapshot(connection: Connection, runtime_role: str) -> dict[str, Any]:
    """Role-normalized effective privileges on schema, tables, and functions."""
    found = connection.scalar(
        text("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=:role)"), {"role": runtime_role}
    )
    if not found:
        raise MigrationValidationError("runtime role does not exist")
    schema = {
        privilege: bool(
            connection.scalar(
                text("SELECT has_schema_privilege(:role, 'public', :privilege)"),
                {"role": runtime_role, "privilege": privilege},
            )
        )
        for privilege in ("USAGE", "CREATE")
    }
    schema_acl = dict(
        connection.execute(
            text("""
                SELECT EXISTS(
                    SELECT 1 FROM aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) acl
                    WHERE acl.grantee=0
                ) AS public_grants,
                ARRAY(
                    SELECT grantee.rolname
                    FROM aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) acl
                         JOIN pg_roles grantee ON grantee.oid=acl.grantee
                    WHERE acl.grantee<>n.nspowner AND grantee.rolname<>:role
                    ORDER BY grantee.rolname
                ) AS other_grantees,
                EXISTS(
                    SELECT 1 FROM aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) acl
                         JOIN pg_roles grantee ON grantee.oid=acl.grantee
                    WHERE grantee.rolname=:role AND acl.is_grantable
                ) AS runtime_grantable
                FROM pg_namespace n WHERE n.nspname='public'
            """),
            {"role": runtime_role},
        )
        .mappings()
        .one()
    )
    names = connection.scalars(
        text("""
            SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
            WHERE n.nspname='public' AND c.relkind IN ('r','p') ORDER BY c.relname
        """)
    ).all()
    tables: dict[str, dict[str, bool]] = {}
    table_acls: dict[str, dict[str, Any]] = {}
    for name in names:
        tables[name] = {
            privilege: bool(
                connection.scalar(
                    text("SELECT has_table_privilege(:role, :table_name, :privilege)"),
                    {
                        "role": runtime_role,
                        "table_name": f"public.{name}",
                        "privilege": privilege,
                    },
                )
            )
            for privilege in (
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
            )
        }
        table_acls[name] = dict(
            connection.execute(
                text("""
                    SELECT EXISTS(
                        SELECT 1
                        FROM aclexplode(coalesce(c.relacl, acldefault('r', c.relowner))) acl
                        WHERE acl.grantee=0
                    ) AS public_grants,
                    ARRAY(
                        SELECT grantee.rolname
                        FROM aclexplode(coalesce(c.relacl, acldefault('r', c.relowner))) acl
                             JOIN pg_roles grantee ON grantee.oid=acl.grantee
                        WHERE acl.grantee<>c.relowner AND grantee.rolname<>:role
                        ORDER BY grantee.rolname
                    ) AS other_grantees,
                    EXISTS(
                        SELECT 1
                        FROM aclexplode(coalesce(c.relacl, acldefault('r', c.relowner))) acl
                             JOIN pg_roles grantee ON grantee.oid=acl.grantee
                        WHERE grantee.rolname=:role AND acl.is_grantable
                    ) AS runtime_grantable
                    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
                    WHERE n.nspname='public' AND c.relname=:name
                """),
                {"role": runtime_role, "name": name},
            )
            .mappings()
            .one()
        )
    functions = [
        dict(row)
        for row in connection.execute(
            text("""
                SELECT p.proname AS name,
                       pg_get_function_identity_arguments(p.oid) AS arguments,
                       has_function_privilege(:role, p.oid, 'EXECUTE') AS runtime_execute,
                       EXISTS (
                           SELECT 1
                           FROM aclexplode(coalesce(p.proacl, acldefault('f', p.proowner))) acl
                           WHERE acl.grantee=0 AND acl.privilege_type='EXECUTE'
                       ) AS public_execute,
                       ARRAY(
                           SELECT grantee.rolname
                           FROM aclexplode(coalesce(p.proacl, acldefault('f', p.proowner))) acl
                                JOIN pg_roles grantee ON grantee.oid=acl.grantee
                           WHERE acl.privilege_type='EXECUTE'
                             AND acl.grantee<>p.proowner
                             AND grantee.rolname<>:role
                           ORDER BY grantee.rolname
                       ) AS other_execute_grantees,
                       EXISTS (
                           SELECT 1
                           FROM aclexplode(coalesce(p.proacl, acldefault('f', p.proowner))) acl
                                JOIN pg_roles grantee ON grantee.oid=acl.grantee
                           WHERE grantee.rolname=:role AND acl.is_grantable
                       ) AS runtime_grantable
                FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
                WHERE n.nspname='public' ORDER BY p.proname, arguments
            """),
            {"role": runtime_role},
        ).mappings()
    ]
    default_table_privileges = connection.scalars(
        text("""
            SELECT acl.privilege_type
            FROM pg_default_acl d JOIN pg_namespace n ON n.oid=d.defaclnamespace
                 CROSS JOIN LATERAL aclexplode(d.defaclacl) acl
                 JOIN pg_roles target_role ON target_role.oid=acl.grantee
            WHERE n.nspname='public' AND d.defaclobjtype='r'
              AND d.defaclrole=(SELECT oid FROM pg_roles WHERE rolname=current_user)
              AND target_role.rolname=:role
            ORDER BY acl.privilege_type
        """),
        {"role": runtime_role},
    ).all()
    default_acl_other_grantees = connection.scalars(
        text("""
            SELECT DISTINCT coalesce(target_role.rolname, 'PUBLIC')
            FROM pg_default_acl d JOIN pg_namespace n ON n.oid=d.defaclnamespace
                 CROSS JOIN LATERAL aclexplode(d.defaclacl) acl
                 LEFT JOIN pg_roles target_role ON target_role.oid=acl.grantee
            WHERE n.nspname='public' AND d.defaclobjtype='r'
              AND d.defaclrole=(SELECT oid FROM pg_roles WHERE rolname=current_user)
              AND acl.grantee<>d.defaclrole
              AND (acl.grantee=0 OR target_role.rolname<>:role)
            ORDER BY 1
        """),
        {"role": runtime_role},
    ).all()
    return {
        "schema": schema,
        "schema_acl": schema_acl,
        "tables": tables,
        "table_acls": table_acls,
        "functions": functions,
        "default_table_privileges": default_table_privileges,
        "default_acl_other_grantees": default_acl_other_grantees,
    }


def validate_runtime_grants(
    connection: Connection, runtime_role: str, *, service_admin_reset_enabled: bool = False
) -> str:
    snapshot = grant_snapshot(connection, runtime_role)
    if snapshot["schema"] != {"USAGE": True, "CREATE": False}:
        raise MigrationValidationError("runtime schema grants differ from baseline")
    if snapshot["schema_acl"] != {
        "public_grants": False,
        "other_grantees": [],
        "runtime_grantable": False,
    }:
        raise MigrationValidationError("unapproved schema grant")
    if snapshot["default_table_privileges"] != ["DELETE", "INSERT", "SELECT", "UPDATE"]:
        raise MigrationValidationError("runtime default table grants differ from baseline")
    if snapshot["default_acl_other_grantees"]:
        raise MigrationValidationError("unapproved default table grant")
    for name, observed in snapshot["tables"].items():
        if snapshot["table_acls"][name] != {
            "public_grants": False,
            "other_grantees": [],
            "runtime_grantable": False,
        }:
            raise MigrationValidationError(f"unapproved table grant: {name}")
        writable: frozenset[str]
        if name in _METADATA_TABLES:
            writable = frozenset()
        elif name in _APPEND_ONLY_TABLES:
            writable = frozenset({"INSERT"})
        else:
            writable = frozenset({"INSERT", "UPDATE", "DELETE"})
        expected = {
            privilege: privilege == "SELECT" or privilege in writable for privilege in observed
        }
        if observed != expected:
            raise MigrationValidationError(f"runtime table grants differ from baseline: {name}")
    for function in snapshot["functions"]:
        if function["other_execute_grantees"]:
            raise MigrationValidationError(f"unapproved function EXECUTE grant: {function['name']}")
        if function["runtime_grantable"]:
            raise MigrationValidationError(
                f"runtime function grant is delegable: {function['name']}"
            )
        if function["name"] == "service_admin_reset_guild_records":
            if function["public_execute"]:
                raise MigrationValidationError("service-admin reset function is public")
            if function["runtime_execute"] != service_admin_reset_enabled:
                raise MigrationValidationError(
                    "service-admin reset function grant differs from policy"
                )
        elif not function["runtime_execute"]:
            raise MigrationValidationError(f"runtime function grant is missing: {function['name']}")
    return _digest(snapshot)


def validate_baseline_catalog(
    connection: Connection,
    runtime_role: str | None = None,
    *,
    service_admin_reset_enabled: bool = False,
) -> str:
    digest = catalog_sha384(connection)
    if digest != BASELINE_CATALOG_SHA384:
        raise MigrationValidationError("database catalog differs from frozen SQLx baseline")
    if runtime_role is not None:
        validate_runtime_grants(
            connection, runtime_role, service_admin_reset_enabled=service_admin_reset_enabled
        )
    return digest


def validate_migration_state(connection: Connection) -> str:
    """Readiness-level history check, without a catalog scan on every probe."""
    _manifest_entries()
    has_sqlx = _table_exists(connection, "_sqlx_migrations")
    has_alembic = _table_exists(connection, "alembic_version")
    has_provenance = _table_exists(connection, "migration_provenance")
    if not has_alembic and not has_provenance:
        validate_sqlx_history(connection)
        return "sqlx_legacy"
    if has_alembic != has_provenance:
        raise MigrationValidationError("Alembic metadata is partially present")
    heads = connection.scalars(text("SELECT version_num FROM public.alembic_version")).all()
    if heads != [BASELINE_REVISION]:
        raise MigrationValidationError("Alembic head differs from approved baseline")
    rows = (
        connection.execute(
            text("""
            SELECT id, source, baseline_revision, baseline_schema_sha384,
                   sqlx_manifest_sha384 FROM public.migration_provenance
        """)
        )
        .mappings()
        .all()
    )
    if len(rows) != 1:
        raise MigrationValidationError("migration provenance is missing or duplicated")
    row = rows[0]
    if (
        row["id"] != 1
        or row["baseline_revision"] != BASELINE_REVISION
        or bytes(row["baseline_schema_sha384"]).hex() != BASELINE_CATALOG_SHA384
        or bytes(row["sqlx_manifest_sha384"]).hex() != SQLX_MANIFEST_SHA384
    ):
        raise MigrationValidationError("migration provenance differs from frozen baseline")
    source = row["source"]
    if source == "sqlx_handover" and has_sqlx:
        validate_sqlx_history(connection)
        return "sqlx_handover"
    if source == "alembic_bootstrap" and not has_sqlx:
        return "alembic_bootstrap"
    raise MigrationValidationError("migration provenance and legacy ledger disagree")


def _config(connection: Connection) -> Config:
    config = Config()
    config.set_main_option("script_location", str(_MIGRATION_DIR))
    config.attributes["connection"] = connection
    return config


def _empty_business_database(connection: Connection) -> bool:
    count = connection.scalar(
        text("""
            SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
            WHERE n.nspname='public' AND c.relkind IN ('r','p','v','m','S','f')
              AND c.relname <> 'alembic_version'
        """)
    )
    functions = connection.scalar(
        text("""
            SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
            WHERE n.nspname='public'
        """)
    )
    return count == 0 and functions == 0 and not _table_exists(connection, "_sqlx_migrations")


def _create_provenance_table(connection: Connection) -> None:
    connection.exec_driver_sql("""
        CREATE TABLE public.migration_provenance (
            id smallint PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            source text NOT NULL CHECK (source IN ('sqlx_handover', 'alembic_bootstrap')),
            baseline_revision varchar(32) NOT NULL,
            baseline_schema_sha384 bytea NOT NULL CHECK (octet_length(baseline_schema_sha384) = 48),
            sqlx_manifest_sha384 bytea NOT NULL CHECK (octet_length(sqlx_manifest_sha384) = 48),
            created_at timestamptz NOT NULL DEFAULT now()
        )
    """)


def _record_provenance(connection: Connection, source: str) -> None:
    connection.execute(
        text("""
            INSERT INTO public.migration_provenance
              (id, source, baseline_revision, baseline_schema_sha384, sqlx_manifest_sha384)
            VALUES (1, :source, :revision, :catalog, :manifest)
        """),
        {
            "source": source,
            "revision": BASELINE_REVISION,
            "catalog": bytes.fromhex(BASELINE_CATALOG_SHA384),
            "manifest": bytes.fromhex(SQLX_MANIFEST_SHA384),
        },
    )


def _bootstrap(connection: Connection) -> None:
    connection.execute(text("SELECT pg_advisory_xact_lock(:key)"), {"key": _ADVISORY_LOCK_KEY})
    if not _empty_business_database(connection):
        raise MigrationValidationError("bootstrap requires a fresh database")
    verify_bootstrap_bundle()
    command.upgrade(_config(connection), "head")
    validate_baseline_catalog(connection)
    if validate_migration_state(connection) != "alembic_bootstrap":
        raise MigrationValidationError("Alembic bootstrap did not record its provenance")


def _handover(connection: Connection, runtime_role: str, service_admin_reset_enabled: bool) -> None:
    connection.execute(text("SELECT pg_advisory_xact_lock(:key)"), {"key": _ADVISORY_LOCK_KEY})
    if _table_exists(connection, "alembic_version") or _table_exists(
        connection, "migration_provenance"
    ):
        raise MigrationValidationError("database already contains Alembic metadata")
    validate_sqlx_history(connection)
    validate_baseline_catalog(
        connection, runtime_role, service_admin_reset_enabled=service_admin_reset_enabled
    )
    _create_provenance_table(connection)
    command.stamp(_config(connection), BASELINE_REVISION)
    _record_provenance(connection, "sqlx_handover")
    if validate_migration_state(connection) != "sqlx_handover":
        raise MigrationValidationError("Alembic handover did not record its provenance")


def _validate(
    connection: Connection, runtime_role: str | None, service_admin_reset_enabled: bool
) -> None:
    validate_migration_state(connection)
    validate_baseline_catalog(
        connection, runtime_role, service_admin_reset_enabled=service_admin_reset_enabled
    )


async def run_command(
    action: str,
    database_url: str,
    runtime_role: str | None = None,
    *,
    service_admin_reset_enabled: bool = False,
) -> None:
    if action == "handover" and not runtime_role:
        raise MigrationValidationError("handover requires --runtime-role for grant validation")
    url = make_url(database_url).set(drivername="postgresql+asyncpg")
    if "sslmode" in url.query:
        mode = url.query["sslmode"]
        url = url.difference_update_query(["sslmode"])
        connect_args: dict[str, Any] = {"ssl": mode}
    else:
        connect_args = {}
    engine = create_async_engine(url, connect_args=connect_args, hide_parameters=True)
    try:
        async with engine.begin() as async_connection:
            await async_connection.exec_driver_sql("SET LOCAL search_path TO public, pg_catalog")
            await async_connection.exec_driver_sql("SET LOCAL lock_timeout = '30s'")
            if action == "bootstrap":
                await async_connection.run_sync(_bootstrap)
            elif action == "handover":
                assert runtime_role is not None
                await async_connection.run_sync(
                    _handover, runtime_role, service_admin_reset_enabled
                )
            elif action == "validate":
                await async_connection.run_sync(
                    _validate, runtime_role, service_admin_reset_enabled
                )
            else:
                raise ValueError(f"unknown migration action: {action}")
    except DBAPIError as error:
        if getattr(error.orig, "sqlstate", None) == "55P03":
            raise MigrationValidationError("migration lock wait timed out") from error
        raise
    finally:
        await engine.dispose()


def main() -> None:
    parser = argparse.ArgumentParser(description="Validate or hand over the frozen SQLx schema")
    parser.add_argument("action", choices=("bootstrap", "handover", "validate"))
    parser.add_argument(
        "--database-url", default=os.environ.get("MONKEYBOT_MIGRATION_DATABASE_URL")
    )
    parser.add_argument("--runtime-role")
    parser.add_argument("--service-admin-reset-enabled", action="store_true")
    args = parser.parse_args()
    if not args.database_url:
        parser.error("--database-url or MONKEYBOT_MIGRATION_DATABASE_URL is required")
    try:
        asyncio.run(
            run_command(
                args.action,
                args.database_url,
                args.runtime_role,
                service_admin_reset_enabled=args.service_admin_reset_enabled,
            )
        )
    except MigrationValidationError as error:
        parser.exit(1, f"migration validation failed: {error}\n")
    print(f"migration {args.action} passed")


if __name__ == "__main__":
    main()
