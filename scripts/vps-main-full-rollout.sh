#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${MONKEYBOT_MAIN_COMPOSE_FILE:-${ROOT_DIR}/deploy/docker-compose.main.yml}"
ENV_FILE="${MONKEYBOT_MAIN_ENV_FILE:-${ROOT_DIR}/deploy/main.env}"
WAIT_TIMEOUT="${MONKEYBOT_FULL_ROLLOUT_WAIT_TIMEOUT:-120}"

usage() {
	printf '%s\n' \
		'Usage: scripts/vps-main-full-rollout.sh <image-tag>' \
		'Run required migrations before this command.' >&2
}

if [[ "$#" -ne 1 ]]; then
	usage
	exit 2
fi

image_tag=$1
if [[ ! "${image_tag}" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]]; then
	printf 'Invalid image tag: %s\n' "${image_tag}" >&2
	exit 2
fi
if [[ ! "${WAIT_TIMEOUT}" =~ ^[1-9][0-9]*$ ]]; then
	printf 'MONKEYBOT_FULL_ROLLOUT_WAIT_TIMEOUT must be a positive integer\n' >&2
	exit 2
fi
if [[ ! -f "${COMPOSE_FILE}" || ! -f "${ENV_FILE}" ]]; then
	printf 'Main deployment files are missing\n' >&2
	exit 2
fi

write_env_value() {
	local file=$1 key=$2 value=$3 temporary_file
	temporary_file="$(mktemp "${file}.XXXXXX")"
	awk -v target_key="${key}" -v replacement="${value}" '
		{
			separator = index($0, "=")
			if (separator > 0 && substr($0, 1, separator - 1) == target_key) {
				print target_key "=" replacement
				found = 1
				next
			}
			print
		}
		END {
			if (!found) print target_key "=" replacement
		}
	' "${file}" >"${temporary_file}"
	chmod --reference="${file}" "${temporary_file}"
	mv "${temporary_file}" "${file}"
}

backup_path="$(mktemp /tmp/monkeybot-main.env.full-rollout.XXXXXX)"
cp -p "${ENV_FILE}" "${backup_path}"
working_env="$(mktemp "${ENV_FILE}.XXXXXX")"
cp -p "${ENV_FILE}" "${working_env}"

write_env_value "${working_env}" MONKEYBOT_RELEASE_TAG "${image_tag}"
for setting in \
	MONKEYBOT_WEB_IMAGE_TAG \
	MONKEYBOT_API_IMAGE_TAG \
	MONKEYBOT_RIOT_API_IMAGE_TAG \
	MONKEYBOT_BOT_IMAGE_TAG; do
	write_env_value "${working_env}" "${setting}" ''
done
mv "${working_env}" "${ENV_FILE}"
set -a
# shellcheck disable=SC1090
. "${ENV_FILE}"
set +a

if ! MONKEYBOT_MAIN_COMPOSE_FILE="${COMPOSE_FILE}" \
	MONKEYBOT_MAIN_ENV_FILE="${ENV_FILE}" \
	MONKEYBOT_CHECK_IMAGES=1 \
	"${ROOT_DIR}/scripts/vps-main-preflight.sh"; then
	cp -p "${backup_path}" "${ENV_FILE}"
	printf 'Preflight failed; restored deployment env from %s\n' "${backup_path}" >&2
	exit 1
fi

if ! docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" pull; then
	cp -p "${backup_path}" "${ENV_FILE}"
	printf 'Image pull failed; restored deployment env from %s\n' "${backup_path}" >&2
	exit 1
fi
if ! docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" up -d --wait \
	--wait-timeout "${WAIT_TIMEOUT}"; then
	printf 'Service recreate failed; rollback env backup: %s\n' "${backup_path}" >&2
	exit 1
fi

docker exec nginx nginx -t
docker exec nginx nginx -s reload

MONKEYBOT_MAIN_COMPOSE_FILE="${COMPOSE_FILE}" \
	MONKEYBOT_MAIN_ENV_FILE="${ENV_FILE}" \
	"${ROOT_DIR}/scripts/vps-main-smoke.sh"

printf 'Full rollout completed: %s\n' "${image_tag}"
printf 'Rollback env backup: %s\n' "${backup_path}"
