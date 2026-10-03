#!/usr/bin/env bash
set -euo pipefail

: "${MONKEYBOT_BACKUP_SOURCE_SERVICE:?set MONKEYBOT_BACKUP_SOURCE_SERVICE to a libpq service}"
: "${MONKEYBOT_RESTORE_TARGET_SERVICE:?set MONKEYBOT_RESTORE_TARGET_SERVICE to a separate empty database service}"
: "${MONKEYBOT_RESTORE_RUNTIME_ROLE:?set MONKEYBOT_RESTORE_RUNTIME_ROLE to the provisioned target runtime role}"

source_runtime_role="${MONKEYBOT_BACKUP_SOURCE_RUNTIME_ROLE:-${MONKEYBOT_RESTORE_RUNTIME_ROLE}}"
service_admin_reset_enabled="${MONKEYBOT_RESTORE_SERVICE_ADMIN_RESET_ENABLED:-false}"
if [[ "${service_admin_reset_enabled}" != false && "${service_admin_reset_enabled}" != true ]]; then
	printf '%s\n' 'MONKEYBOT_RESTORE_SERVICE_ADMIN_RESET_ENABLED must be true or false' >&2
	exit 2
fi

if [[ "${MONKEYBOT_BACKUP_SOURCE_SERVICE}" == "${MONKEYBOT_RESTORE_TARGET_SERVICE}" ]]; then
	printf '%s\n' 'source and restore target must use different libpq services' >&2
	exit 2
fi

for command in pg_dump pg_restore psql sha384sum; do
	if ! command -v "${command}" >/dev/null 2>&1; then
		printf 'required command is missing: %s\n' "${command}" >&2
		exit 2
	fi
done

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
migration_dir="${MONKEYBOT_MIGRATION_DIR:-${repo_root}/apps/api/migrations}"
shopt -s nullglob
migration_files=("${migration_dir}"/*.sql)
shopt -u nullglob

if [[ ${#migration_files[@]} -eq 0 ]]; then
	printf 'no SQLx migrations found in %s\n' "${migration_dir}" >&2
	exit 2
fi
if [[ ${#migration_files[@]} -ne 26 ]]; then
	printf 'expected exactly 26 frozen SQLx migrations, found %s\n' "${#migration_files[@]}" >&2
	exit 2
fi

expected_migrations=()
expected_version=1
for migration_file in "${migration_files[@]}"; do
	filename="$(basename -- "${migration_file}")"
	version_prefix="${filename%%_*}"
	if [[ ! "${version_prefix}" =~ ^[0-9]+$ ]]; then
		printf 'migration filename must start with a numeric version: %s\n' "${filename}" >&2
		exit 2
	fi
	version="$((10#${version_prefix}))"
	if [[ "${version}" -ne "${expected_version}" ]]; then
		printf 'SQLx migrations must be exact versions 0001..0026: %s\n' "${filename}" >&2
		exit 2
	fi
	((expected_version += 1))
	checksum="$(sha384sum "${migration_file}")"
	checksum="${checksum%% *}"
	expected_migrations+=("${version}:${checksum}")
done

expected_migration_fingerprint="$(
	IFS=,
	printf 'true|%s' "${expected_migrations[*]}"
)"
manifest_file="${repo_root}/apps/api-python/src/monkeybot_api/db/sqlx-manifest.json"
if [[ ! -f "${manifest_file}" ]]; then
	printf 'SQLx manifest is missing: %s\n' "${manifest_file}" >&2
	exit 2
fi
expected_manifest_sha384='894000becec56f4661e1e0d7272d2716079774058d2b0426fcad877404eb2a7271e4696443468eff09f417a3deee120d'
actual_manifest_sha384="$(sha384sum "${manifest_file}")"
actual_manifest_sha384="${actual_manifest_sha384%% *}"
if [[ "${actual_manifest_sha384}" != "${expected_manifest_sha384}" ]]; then
	printf '%s\n' 'packaged SQLx manifest differs from frozen 0001..0026' >&2
	exit 2
fi
expected_baseline_revision='20261003_baseline'
# Frozen SQLx 0001..0026 catalog hash; keep in sync with migration_handover.py.
expected_baseline_schema_sha384='9fa120522e6f6a7b791a6bdab539860d1ee7605f49efcb1a91a49850a38462573f56784462d3d507b6c47a4b9b85b5e6'

migration_shape_sql="
SELECT
  (CASE WHEN to_regclass('public._sqlx_migrations') IS NULL THEN '0' ELSE '1' END) ||
  (CASE WHEN to_regclass('public.alembic_version') IS NULL THEN '0' ELSE '1' END) ||
  (CASE WHEN to_regclass('public.migration_provenance') IS NULL THEN '0' ELSE '1' END)
"

migration_fingerprint_sql="
SELECT concat_ws('|',
  COALESCE(bool_and(success), false)::text,
  COALESCE(string_agg(
    version::text || ':' || encode(checksum, 'hex'),
    ',' ORDER BY version
  ), '')
)
FROM public._sqlx_migrations
"

database_migration_fingerprint() {
	PGSERVICE="$1" psql --no-psqlrc --tuples-only --no-align --set ON_ERROR_STOP=1 \
		--command "${migration_fingerprint_sql}" | tr -d '[:space:]'
}

alembic_fingerprint_sql="
SELECT count(*)::text || '|' || COALESCE(string_agg(version_num, ',' ORDER BY version_num), '')
FROM public.alembic_version
"

provenance_fingerprint_sql="
SELECT count(*)::text || '|' || COALESCE(string_agg(
  id::text || ':' || source || ':' || baseline_revision || ':' ||
  encode(baseline_schema_sha384, 'hex') || ':' ||
  encode(sqlx_manifest_sha384, 'hex') || ':' ||
  to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US'),
  ',' ORDER BY id
), '')
FROM public.migration_provenance
"

database_scalar() {
	PGSERVICE="$1" psql --no-psqlrc --tuples-only --no-align --set ON_ERROR_STOP=1 \
		--command "$2" | tr -d '[:space:]'
}

role_exists() {
	printf '%s\n' "SELECT count(*) FROM pg_roles WHERE rolname = :'runtime_role';" |
		PGSERVICE="$1" psql --no-psqlrc --tuples-only --no-align --set ON_ERROR_STOP=1 \
			--set "runtime_role=$2" | tr -d '[:space:]'
}

if [[ "$(role_exists "${MONKEYBOT_BACKUP_SOURCE_SERVICE}" "${source_runtime_role}")" != 1 ]]; then
	printf 'source runtime role is not provisioned: %s\n' "${source_runtime_role}" >&2
	exit 2
fi
if [[ "$(role_exists "${MONKEYBOT_RESTORE_TARGET_SERVICE}" "${MONKEYBOT_RESTORE_RUNTIME_ROLE}")" != 1 ]]; then
	printf 'restore target runtime role is not provisioned: %s\n' \
		"${MONKEYBOT_RESTORE_RUNTIME_ROLE}" >&2
	exit 2
fi

grant_status_sql="
WITH table_grants AS (
  SELECT count(*) AS table_count, COALESCE(bool_and(
    has_table_privilege(:'runtime_role', format('public.%I', c.relname), 'SELECT')
    AND has_table_privilege(:'runtime_role', format('public.%I', c.relname), 'INSERT') =
      (c.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance'))
    AND has_table_privilege(:'runtime_role', format('public.%I', c.relname), 'UPDATE') =
      (c.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance',
                         'auction_actions', 'auction_point_ledger', 'mmr_ledger_entries',
                         'admin_audit_log', 'dummy_user_audit_log'))
    AND has_table_privilege(:'runtime_role', format('public.%I', c.relname), 'DELETE') =
      (c.relname NOT IN ('_sqlx_migrations', 'alembic_version', 'migration_provenance',
                         'auction_actions', 'auction_point_ledger', 'mmr_ledger_entries',
                         'admin_audit_log', 'dummy_user_audit_log'))
    AND NOT has_table_privilege(:'runtime_role', format('public.%I', c.relname), 'TRUNCATE')
    AND NOT has_table_privilege(:'runtime_role', format('public.%I', c.relname), 'REFERENCES')
    AND NOT has_table_privilege(:'runtime_role', format('public.%I', c.relname), 'TRIGGER')
  ), false) AS valid
  FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
  WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p')
), function_grants AS (
  SELECT count(*) AS function_count, COALESCE(bool_and(
    CASE WHEN p.oid = to_regprocedure('public.service_admin_reset_guild_records(text)')
      THEN has_function_privilege(:'runtime_role', p.oid, 'EXECUTE') =
        (:'service_admin_reset_enabled')::boolean
      ELSE has_function_privilege(:'runtime_role', p.oid, 'EXECUTE')
    END
  ), false) AS valid
  FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = 'public'
), reset_acl AS (
  SELECT NOT EXISTS (
    SELECT 1 FROM pg_proc p
      CROSS JOIN LATERAL aclexplode(COALESCE(p.proacl, acldefault('f', p.proowner))) acl
    WHERE p.oid = to_regprocedure('public.service_admin_reset_guild_records(text)')
      AND acl.privilege_type = 'EXECUTE'
      AND acl.grantee <> p.proowner
      AND NOT ((:'service_admin_reset_enabled')::boolean
        AND acl.grantee = (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role'))
  ) AS valid
), unexpected_acl AS (
  SELECT EXISTS (
    SELECT 1 FROM pg_namespace n
      CROSS JOIN LATERAL aclexplode(COALESCE(n.nspacl, acldefault('n', n.nspowner))) acl
    WHERE n.nspname = 'public'
      AND (acl.grantee = 0
        OR (acl.grantee <> n.nspowner
          AND acl.grantee <> (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role'))
        OR (acl.grantee = (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role')
          AND acl.is_grantable))
    UNION ALL
    SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
      CROSS JOIN LATERAL aclexplode(COALESCE(c.relacl, acldefault('r', c.relowner))) acl
    WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p')
      AND (acl.grantee = 0
        OR (acl.grantee <> c.relowner
          AND acl.grantee <> (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role'))
        OR (acl.grantee = (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role')
          AND acl.is_grantable))
    UNION ALL
    SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
      CROSS JOIN LATERAL aclexplode(COALESCE(p.proacl, acldefault('f', p.proowner))) acl
    WHERE n.nspname = 'public'
      AND ((acl.grantee <> 0 AND acl.grantee <> p.proowner
        AND acl.grantee <> (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role'))
        OR (acl.grantee = (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role')
          AND acl.is_grantable))
  ) AS found
), default_table_acl AS (
  SELECT
    ARRAY(
      SELECT acl.privilege_type::text
      FROM pg_default_acl d JOIN pg_namespace n ON n.oid = d.defaclnamespace
        CROSS JOIN LATERAL aclexplode(d.defaclacl) acl
      WHERE n.nspname = 'public' AND d.defaclobjtype = 'r'
        AND d.defaclrole = (SELECT relowner FROM pg_class
          WHERE oid = to_regclass('public.users'))
        AND acl.grantee = (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role')
      ORDER BY acl.privilege_type
    ) = ARRAY['DELETE', 'INSERT', 'SELECT', 'UPDATE']
    AND NOT EXISTS (
      SELECT 1 FROM pg_default_acl d JOIN pg_namespace n ON n.oid = d.defaclnamespace
        CROSS JOIN LATERAL aclexplode(d.defaclacl) acl
      WHERE n.nspname = 'public' AND d.defaclobjtype = 'r'
        AND d.defaclrole = (SELECT relowner FROM pg_class
          WHERE oid = to_regclass('public.users'))
        AND (acl.grantee NOT IN (d.defaclrole,
          (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role'))
          OR (acl.grantee = (SELECT oid FROM pg_roles WHERE rolname = :'runtime_role')
            AND acl.is_grantable))
    ) AS valid
)
SELECT
  (CASE WHEN has_schema_privilege(:'runtime_role', 'public', 'USAGE')
            AND NOT has_schema_privilege(:'runtime_role', 'public', 'CREATE')
    THEN 'ok' ELSE 'schema' END) || '|' ||
  (CASE WHEN table_count > 0 AND table_grants.valid
    THEN 'ok' ELSE 'tables' END) || '|' ||
  (CASE WHEN function_count > 0 AND function_grants.valid
    THEN 'ok' ELSE 'functions' END) || '|' ||
  (CASE WHEN to_regprocedure('public.service_admin_reset_guild_records(text)') IS NOT NULL
            AND reset_acl.valid
    THEN 'ok' ELSE 'reset_acl' END) || '|' ||
  (CASE WHEN NOT unexpected_acl.found
    THEN 'ok' ELSE 'extra_acl' END) || '|' ||
  (CASE WHEN default_table_acl.valid
    THEN 'ok' ELSE 'default_acl' END)
FROM table_grants, function_grants, reset_acl, unexpected_acl, default_table_acl
"

database_grant_status() {
	printf '%s\n' "${grant_status_sql};" |
		PGSERVICE="$1" psql --no-psqlrc --tuples-only --no-align --set ON_ERROR_STOP=1 \
			--set "runtime_role=$2" \
			--set "service_admin_reset_enabled=${service_admin_reset_enabled}" |
		tr -d '[:space:]'
}

validate_migration_state() {
	local side="$1" shape="$2" sqlx="$3" alembic="$4" provenance="$5"
	local expected_provenance_prefix migration_source migration_state
	case "${shape}" in
		100)
			if [[ "${sqlx}" != "${expected_migration_fingerprint}" ]]; then
				printf '%s SQLx migration state does not match repository migrations\n' "${side}" >&2
				return 1
			fi
			printf '%s' sqlx_only
			;;
		111|011)
			if [[ "${alembic}" != "1|${expected_baseline_revision}" ]]; then
				printf '%s Alembic head does not match the baseline revision\n' "${side}" >&2
				return 1
			fi
			if [[ "${shape}" == 111 ]]; then
				if [[ "${sqlx}" != "${expected_migration_fingerprint}" ]]; then
					printf '%s SQLx migration state does not match repository migrations\n' "${side}" >&2
					return 1
				fi
				migration_source=sqlx_handover
				migration_state=sqlx_handover
			else
				migration_source=alembic_bootstrap
				migration_state=alembic_bootstrap
			fi
			expected_provenance_prefix="1|1:${migration_source}:${expected_baseline_revision}:${expected_baseline_schema_sha384}:${expected_manifest_sha384}:"
			if [[ "${provenance}" != "${expected_provenance_prefix}"* ]]; then
				printf '%s migration provenance does not match the approved baseline\n' "${side}" >&2
				return 1
			fi
			printf '%s' "${migration_state}"
			;;
		*)
			printf '%s migration metadata is missing or partially installed (shape=%s)\n' \
				"${side}" "${shape}" >&2
			return 1
			;;
	esac
}

target_table_count="$({
	PGSERVICE="${MONKEYBOT_RESTORE_TARGET_SERVICE}" psql --no-psqlrc --tuples-only --no-align \
		--set ON_ERROR_STOP=1 \
		--command "SELECT count(*) FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE'"
} | tr -d '[:space:]')"

if [[ "${target_table_count}" != "0" ]]; then
	printf '%s\n' 'restore target is not empty; refusing to overwrite it' >&2
	exit 2
fi

dump_file="$(mktemp --suffix=.monkeybot.dump)"
snapshot_pid=''
snapshot_input_fd=''

cleanup() {
	if [[ -n "${snapshot_pid}" ]]; then
		printf '%s\n' 'ROLLBACK;' '\q' >&"${snapshot_input_fd}" 2>/dev/null || true
		wait "${snapshot_pid}" 2>/dev/null || true
	fi
	rm -f "${dump_file}"
}
trap cleanup EXIT

signature_sql="
SELECT concat_ws('|',
  (SELECT count(*) FROM information_schema.tables
    WHERE table_schema='public' AND table_type='BASE TABLE'),
  (SELECT count(*) FROM public.users),
  (SELECT count(*) FROM public.guilds),
  (SELECT count(*) FROM public.games),
  (SELECT count(*) FROM public.mmr_ledger_entries),
  (SELECT count(*) FROM public.admin_audit_log),
  (SELECT count(*) FROM public.outbox_events)
)"

coproc SOURCE_SNAPSHOT {
	PGSERVICE="${MONKEYBOT_BACKUP_SOURCE_SERVICE}" psql \
		--no-psqlrc \
		--tuples-only \
		--no-align \
		--quiet \
		--set ON_ERROR_STOP=1 \
		--set "runtime_role=${source_runtime_role}" \
		--set "service_admin_reset_enabled=${service_admin_reset_enabled}"
}
snapshot_pid="${SOURCE_SNAPSHOT_PID}"
snapshot_output_fd="${SOURCE_SNAPSHOT[0]}"
snapshot_input_fd="${SOURCE_SNAPSHOT[1]}"

printf '%s\n' \
	'BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;' \
	'SELECT pg_export_snapshot();' \
	"${signature_sql};" \
	"${migration_shape_sql};" \
	"${grant_status_sql};" >&"${snapshot_input_fd}"

IFS= read -r snapshot_id <&"${snapshot_output_fd}"
IFS= read -r source_signature <&"${snapshot_output_fd}"
IFS= read -r source_shape <&"${snapshot_output_fd}"
IFS= read -r source_grant_status <&"${snapshot_output_fd}"
source_signature="$(tr -d '[:space:]' <<<"${source_signature}")"
source_shape="$(tr -d '[:space:]' <<<"${source_shape}")"
source_grant_status="$(tr -d '[:space:]' <<<"${source_grant_status}")"
if [[ "${source_grant_status}" != 'ok|ok|ok|ok|ok|ok' ]]; then
	printf 'source runtime grants are unsafe: %s\n' "${source_grant_status}" >&2
	exit 1
fi

source_migration_fingerprint='absent'
source_alembic_fingerprint='absent'
source_provenance_fingerprint='absent'
if [[ "${source_shape}" == 100 || "${source_shape}" == 111 ]]; then
	printf '%s\n' "${migration_fingerprint_sql};" >&"${snapshot_input_fd}"
	IFS= read -r source_migration_fingerprint <&"${snapshot_output_fd}"
	source_migration_fingerprint="$(tr -d '[:space:]' <<<"${source_migration_fingerprint}")"
fi
if [[ "${source_shape}" == 111 || "${source_shape}" == 011 ]]; then
	printf '%s\n' "${alembic_fingerprint_sql};" "${provenance_fingerprint_sql};" \
		>&"${snapshot_input_fd}"
	IFS= read -r source_alembic_fingerprint <&"${snapshot_output_fd}"
	IFS= read -r source_provenance_fingerprint <&"${snapshot_output_fd}"
	source_alembic_fingerprint="$(tr -d '[:space:]' <<<"${source_alembic_fingerprint}")"
	source_provenance_fingerprint="$(tr -d '[:space:]' <<<"${source_provenance_fingerprint}")"
fi
source_state="$(validate_migration_state source "${source_shape}" \
	"${source_migration_fingerprint}" "${source_alembic_fingerprint}" \
	"${source_provenance_fingerprint}")"

PGSERVICE="${MONKEYBOT_BACKUP_SOURCE_SERVICE}" pg_dump \
	--format=custom \
	--no-owner \
	--no-acl \
	--snapshot="${snapshot_id}" \
	--file "${dump_file}"

printf '%s\n' 'ROLLBACK;' '\q' >&"${snapshot_input_fd}"
wait "${snapshot_pid}"
snapshot_pid=''

PGSERVICE="${MONKEYBOT_RESTORE_TARGET_SERVICE}" pg_restore \
	--exit-on-error \
	--no-owner \
	--no-acl \
	--dbname="service=${MONKEYBOT_RESTORE_TARGET_SERVICE}" \
	"${dump_file}"

# --no-acl omits function and table grants; the isolated target is not runtime-ready
# until migration-owner grants are reapplied and effective privileges are verified.
PGSERVICE="${MONKEYBOT_RESTORE_TARGET_SERVICE}" psql \
	--no-psqlrc --set ON_ERROR_STOP=1 \
	--set "runtime_role=${MONKEYBOT_RESTORE_RUNTIME_ROLE}" \
	--set "service_admin_reset_enabled=${service_admin_reset_enabled}" \
	--file "${repo_root}/deploy/postgres/runtime-grants.sql" >/dev/null
target_grant_status="$(database_grant_status "${MONKEYBOT_RESTORE_TARGET_SERVICE}" \
	"${MONKEYBOT_RESTORE_RUNTIME_ROLE}")"
if [[ "${target_grant_status}" != 'ok|ok|ok|ok|ok|ok' ]]; then
	printf 'restored runtime grants are unsafe: %s\n' "${target_grant_status}" >&2
	exit 1
fi

target_signature="$(PGSERVICE="${MONKEYBOT_RESTORE_TARGET_SERVICE}" \
	psql --no-psqlrc --tuples-only --no-align --set ON_ERROR_STOP=1 \
	--command "${signature_sql}" | tr -d '[:space:]')"

target_shape="$(database_scalar "${MONKEYBOT_RESTORE_TARGET_SERVICE}" "${migration_shape_sql}")"
if [[ "${target_shape}" != "${source_shape}" ]]; then
	printf 'restored migration metadata shape mismatch: source=%s target=%s\n' \
		"${source_shape}" "${target_shape}" >&2
	exit 1
fi
target_migration_fingerprint='absent'
target_alembic_fingerprint='absent'
target_provenance_fingerprint='absent'
if [[ "${target_shape}" == 100 || "${target_shape}" == 111 ]]; then
	target_migration_fingerprint="$(database_migration_fingerprint "${MONKEYBOT_RESTORE_TARGET_SERVICE}")"
fi
if [[ "${target_shape}" == 111 || "${target_shape}" == 011 ]]; then
	target_alembic_fingerprint="$(database_scalar "${MONKEYBOT_RESTORE_TARGET_SERVICE}" "${alembic_fingerprint_sql}")"
	target_provenance_fingerprint="$(database_scalar "${MONKEYBOT_RESTORE_TARGET_SERVICE}" "${provenance_fingerprint_sql}")"
fi
target_state="$(validate_migration_state restored "${target_shape}" \
	"${target_migration_fingerprint}" "${target_alembic_fingerprint}" \
	"${target_provenance_fingerprint}")"

if [[ "${source_state}" != "${target_state}" ||
	"${source_migration_fingerprint}" != "${target_migration_fingerprint}" ||
	"${source_alembic_fingerprint}" != "${target_alembic_fingerprint}" ||
	"${source_provenance_fingerprint}" != "${target_provenance_fingerprint}" ]]; then
	printf '%s\n' 'restored migration provenance differs from the source snapshot' >&2
	exit 1
fi

if [[ "${source_signature}" != "${target_signature}" ]]; then
	printf 'restore signature mismatch: source=%s target=%s\n' \
		"${source_signature}" "${target_signature}" >&2
	exit 1
fi

sqlx_migration_count=0
if [[ "${target_shape}" == 100 || "${target_shape}" == 111 ]]; then
	sqlx_migration_count="${#migration_files[@]}"
fi
printf 'PostgreSQL restore smoke passed: state=%s sqlx_migrations=%s signature=%s\n' \
	"${target_state}" "${sqlx_migration_count}" "${target_signature}"
