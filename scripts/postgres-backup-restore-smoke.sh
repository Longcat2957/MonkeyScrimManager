#!/usr/bin/env bash
set -euo pipefail

: "${MONKEYBOT_BACKUP_SOURCE_SERVICE:?set MONKEYBOT_BACKUP_SOURCE_SERVICE to a libpq service}"
: "${MONKEYBOT_RESTORE_TARGET_SERVICE:?set MONKEYBOT_RESTORE_TARGET_SERVICE to a separate empty database service}"

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

expected_migrations=()
for migration_file in "${migration_files[@]}"; do
	filename="$(basename -- "${migration_file}")"
	version_prefix="${filename%%_*}"
	if [[ ! "${version_prefix}" =~ ^[0-9]+$ ]]; then
		printf 'migration filename must start with a numeric version: %s\n' "${filename}" >&2
		exit 2
	fi
	version="$((10#${version_prefix}))"
	checksum="$(sha384sum "${migration_file}")"
	checksum="${checksum%% *}"
	expected_migrations+=("${version}:${checksum}")
done

expected_migration_fingerprint="$(
	IFS=,
	printf 'true|%s' "${expected_migrations[*]}"
)"

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
		--set ON_ERROR_STOP=1
}
snapshot_pid="${SOURCE_SNAPSHOT_PID}"
snapshot_output_fd="${SOURCE_SNAPSHOT[0]}"
snapshot_input_fd="${SOURCE_SNAPSHOT[1]}"

printf '%s\n' \
	'BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;' \
	'SELECT pg_export_snapshot();' \
	"${signature_sql};" \
	"${migration_fingerprint_sql};" >&"${snapshot_input_fd}"

IFS= read -r snapshot_id <&"${snapshot_output_fd}"
IFS= read -r source_signature <&"${snapshot_output_fd}"
IFS= read -r source_migration_fingerprint <&"${snapshot_output_fd}"
source_signature="$(tr -d '[:space:]' <<<"${source_signature}")"
source_migration_fingerprint="$(tr -d '[:space:]' <<<"${source_migration_fingerprint}")"

if [[ "${source_migration_fingerprint}" != "${expected_migration_fingerprint}" ]]; then
	printf '%s\n' 'source migration state does not match repository migrations' >&2
	exit 1
fi

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

target_signature="$(PGSERVICE="${MONKEYBOT_RESTORE_TARGET_SERVICE}" \
	psql --no-psqlrc --tuples-only --no-align --set ON_ERROR_STOP=1 \
	--command "${signature_sql}" | tr -d '[:space:]')"

target_migration_fingerprint="$(database_migration_fingerprint "${MONKEYBOT_RESTORE_TARGET_SERVICE}")"
if [[ "${target_migration_fingerprint}" != "${expected_migration_fingerprint}" ]]; then
	printf '%s\n' 'restored migration state does not match repository migrations' >&2
	exit 1
fi

if [[ "${source_signature}" != "${target_signature}" ]]; then
	printf 'restore signature mismatch: source=%s target=%s\n' \
		"${source_signature}" "${target_signature}" >&2
	exit 1
fi

printf 'PostgreSQL restore smoke passed: migrations=%s signature=%s\n' \
	"${#migration_files[@]}" "${target_signature}"
