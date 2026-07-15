#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${MONKEYBOT_DEV_COMPOSE_FILE:-${ROOT_DIR}/deploy/docker-compose.dev.yml}"
ENV_FILE="${MONKEYBOT_DEV_ENV_FILE:-${ROOT_DIR}/deploy/dev.env}"
CHECK_IMAGES="${MONKEYBOT_CHECK_IMAGES:-0}"

if [[ ! -f "${COMPOSE_FILE}" ]]; then
	printf 'Compose file does not exist: %s\n' "${COMPOSE_FILE}" >&2
	exit 2
fi

if [[ ! -f "${ENV_FILE}" ]]; then
	printf 'Deployment env file does not exist: %s\n' "${ENV_FILE}" >&2
	exit 2
fi

# This file is deployment wiring only. Secrets stay in the runtime env files it points to.
set -a
# shellcheck disable=SC1090
. "${ENV_FILE}"
set +a

required=(
	MONKEYBOT_RELEASE_TAG
	MONKEYBOT_WEB_IMAGE
	MONKEYBOT_API_IMAGE
	MONKEYBOT_RIOT_API_IMAGE
	MONKEYBOT_BOT_IMAGE
	MONKEYBOT_PUBLIC_ORIGIN
	MONKEYBOT_WEB_PORT
	MONKEYBOT_API_PORT
	MONKEYBOT_BOT_HEALTH_PORT
	MONKEYBOT_EDGE_NETWORK
	MONKEYBOT_DATA_NETWORK
	MONKEYBOT_API_RUNTIME_ENV
	MONKEYBOT_MIGRATION_RUNTIME_ENV
	MONKEYBOT_RIOT_RUNTIME_ENV
	MONKEYBOT_BOT_RUNTIME_ENV
)

for name in "${required[@]}"; do
	if [[ -z "${!name:-}" ]]; then
		printf 'Missing required deployment setting: %s\n' "${name}" >&2
		exit 2
	fi
done

if [[ "${MONKEYBOT_RELEASE_TAG}" == replace-with-* ]]; then
	printf 'MONKEYBOT_RELEASE_TAG is still a placeholder\n' >&2
	exit 2
fi

compose_dir="$(cd "$(dirname "${COMPOSE_FILE}")" && pwd)"
resolve_compose_path() {
	local path=$1
	if [[ "${path}" == /* ]]; then
		printf '%s\n' "${path}"
	else
		printf '%s/%s\n' "${compose_dir}" "${path#./}"
	fi
}

for network in "${MONKEYBOT_EDGE_NETWORK}" "${MONKEYBOT_DATA_NETWORK}"; do
	if ! docker network inspect "${network}" >/dev/null 2>&1; then
		printf 'Required Docker network does not exist: %s\n' "${network}" >&2
		exit 1
	fi
done

runtime_files=()
for runtime_setting in \
	"${MONKEYBOT_API_RUNTIME_ENV}" \
	"${MONKEYBOT_MIGRATION_RUNTIME_ENV}" \
	"${MONKEYBOT_RIOT_RUNTIME_ENV}" \
	"${MONKEYBOT_BOT_RUNTIME_ENV}"; do
	runtime_file="$(resolve_compose_path "${runtime_setting}")"
	if [[ ! -f "${runtime_file}" ]]; then
		printf 'Required runtime env file does not exist: %s\n' "${runtime_file}" >&2
		exit 1
	fi
	mode="$(stat -c '%a' "${runtime_file}")"
	if [[ "${mode}" != "600" ]]; then
		printf 'Runtime env file must have mode 600: %s (got %s)\n' "${runtime_file}" "${mode}" >&2
		exit 1
	fi
	runtime_files+=("${runtime_file}")
done

api_runtime_file="${runtime_files[0]}"
migration_runtime_file="${runtime_files[1]}"
riot_runtime_file="${runtime_files[2]}"
bot_runtime_file="${runtime_files[3]}"

env_value() {
	local file=$1
	local key=$2
	awk -v target="${key}" '
		/^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
		{
			separator = index($0, "=")
			if (separator == 0) next
			name = substr($0, 1, separator - 1)
			sub(/^[[:space:]]+/, "", name)
			sub(/[[:space:]]+$/, "", name)
			if (name != target) next
			value = substr($0, separator + 1)
			sub(/\r$/, "", value)
			print value
			exit
		}
	' "${file}"
}

require_runtime_keys() {
	local file=$1
	shift
	for key in "$@"; do
		if [[ -z "$(env_value "${file}" "${key}")" ]]; then
			printf 'Required runtime setting is missing or empty: %s in %s\n' "${key}" "${file}" >&2
			exit 1
		fi
	done
}

require_runtime_keys "${api_runtime_file}" \
	MONKEYBOT_DATABASE_URL \
	MONKEYBOT_DISCORD_CLIENT_ID \
	MONKEYBOT_DISCORD_CLIENT_SECRET \
	MONKEYBOT_OAUTH_ENCRYPTION_KEY \
	MONKEYBOT_SESSION_TTL_SECONDS \
	MONKEYBOT_SERVICE_ADMIN_IDS \
	MONKEYBOT_BOT_SERVICE_TOKEN \
	MONKEYBOT_RIOT_SERVICE_TOKEN
require_runtime_keys "${migration_runtime_file}" MONKEYBOT_MIGRATION_DATABASE_URL
require_runtime_keys "${riot_runtime_file}" \
	MONKEYBOT_RIOT_API_KEY \
	MONKEYBOT_RIOT_INTERNAL_TOKEN \
	MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS
require_runtime_keys "${bot_runtime_file}" \
	MONKEYBOT_BOT_TOKEN \
	MONKEYBOT_BOT_APPLICATION_ID \
	MONKEYBOT_BOT_SERVICE_TOKEN \
	MONKEYBOT_BOT_INSTANCE_ID

reject_placeholder_values() {
	local file=$1
	shift
	for key in "$@"; do
		local value
		value="$(env_value "${file}" "${key}")"
		if [[ "${value}" == replace-with-* || "${value}" =~ ^0+$ ]]; then
			printf 'Runtime setting still contains an example value: %s in %s\n' "${key}" "${file}" >&2
			exit 1
		fi
	done
}

reject_placeholder_values "${api_runtime_file}" \
	MONKEYBOT_DATABASE_URL \
	MONKEYBOT_DISCORD_CLIENT_ID \
	MONKEYBOT_DISCORD_CLIENT_SECRET \
	MONKEYBOT_OAUTH_ENCRYPTION_KEY \
	MONKEYBOT_SERVICE_ADMIN_IDS \
	MONKEYBOT_BOT_SERVICE_TOKEN \
	MONKEYBOT_RIOT_SERVICE_TOKEN
reject_placeholder_values "${migration_runtime_file}" MONKEYBOT_MIGRATION_DATABASE_URL
reject_placeholder_values "${riot_runtime_file}" \
	MONKEYBOT_RIOT_API_KEY \
	MONKEYBOT_RIOT_INTERNAL_TOKEN
reject_placeholder_values "${bot_runtime_file}" \
	MONKEYBOT_BOT_TOKEN \
	MONKEYBOT_BOT_APPLICATION_ID \
	MONKEYBOT_BOT_SERVICE_TOKEN \
	MONKEYBOT_BOT_INSTANCE_ID

if [[ "$(env_value "${api_runtime_file}" MONKEYBOT_RIOT_SERVICE_TOKEN)" != "$(env_value "${riot_runtime_file}" MONKEYBOT_RIOT_INTERNAL_TOKEN)" ]]; then
	printf '%s\n' 'Riot internal tokens do not match between Axum and Riot runtime files' >&2
	exit 1
fi

if [[ "$(env_value "${api_runtime_file}" MONKEYBOT_BOT_SERVICE_TOKEN)" != "$(env_value "${bot_runtime_file}" MONKEYBOT_BOT_SERVICE_TOKEN)" ]]; then
	printf '%s\n' 'Bot service tokens do not match between Axum and Bot runtime files' >&2
	exit 1
fi

docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" config -q

if [[ "${CHECK_IMAGES}" == "1" ]]; then
	for image in \
		"${MONKEYBOT_WEB_IMAGE}" \
		"${MONKEYBOT_API_IMAGE}" \
		"${MONKEYBOT_RIOT_API_IMAGE}" \
		"${MONKEYBOT_BOT_IMAGE}"; do
		docker manifest inspect "${image}:${MONKEYBOT_RELEASE_TAG}" >/dev/null
	done
fi

printf 'MonkeyBot dev preflight passed for release %s\n' "${MONKEYBOT_RELEASE_TAG}"
