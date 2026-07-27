#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${MONKEYBOT_DEV_COMPOSE_FILE:-${ROOT_DIR}/deploy/docker-compose.dev.yml}"
ENV_FILE="${MONKEYBOT_DEV_ENV_FILE:-${ROOT_DIR}/deploy/dev.env}"
CHECK_IMAGES="${MONKEYBOT_CHECK_IMAGES:-0}"
DEPLOYMENT_NAME="${MONKEYBOT_DEPLOYMENT_NAME:-dev}"
INFRA_ROOT="${MONKEYBOT_INFRA_ROOT:-/root/monkeybot-infra}"
INFRA_ROOT="${INFRA_ROOT%/}"

case "${DEPLOYMENT_NAME}" in
	dev|main) ;;
	*)
		printf 'Unsupported deployment identity: %s\n' "${DEPLOYMENT_NAME}" >&2
		exit 2
		;;
esac

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
	MONKEYBOT_DISCORD_APPLICATION_ID
	MONKEYBOT_BOT_COMMAND_SCOPE
	MONKEYBOT_BOT_INSTANCE_ID
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

component_image_tag() {
	local component=$1 setting configured_tag
	case "${component}" in
		web) setting=MONKEYBOT_WEB_IMAGE_TAG ;;
		api) setting=MONKEYBOT_API_IMAGE_TAG ;;
		riot-api) setting=MONKEYBOT_RIOT_API_IMAGE_TAG ;;
		bot) setting=MONKEYBOT_BOT_IMAGE_TAG ;;
		*)
			printf 'Unsupported image component: %s\n' "${component}" >&2
			exit 2
			;;
	esac
	configured_tag="${!setting-}"
	printf '%s\n' "${configured_tag:-${MONKEYBOT_RELEASE_TAG}}"
}

validate_image_tag() {
	local setting=$1 value=$2
	if [[ "${value}" == replace-with-* || ! "${value}" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]]; then
		printf 'Invalid image tag for %s\n' "${setting}" >&2
		exit 2
	fi
}

validate_image_tag MONKEYBOT_RELEASE_TAG "${MONKEYBOT_RELEASE_TAG}"
for component in web api riot-api bot; do
	validate_image_tag "${component}" "$(component_image_tag "${component}")"
done

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
	if [[ "${runtime_setting}" != "${INFRA_ROOT}"/"${DEPLOYMENT_NAME}"/* ]]; then
		printf 'Runtime env path must stay inside the %s environment directory: %s\n' \
			"${DEPLOYMENT_NAME}" "${runtime_setting}" >&2
		exit 1
	fi
	runtime_file="$(resolve_compose_path "${runtime_setting}")"
	if [[ ! -f "${runtime_file}" ]]; then
		printf 'Required runtime env file does not exist: %s\n' "${runtime_file}" >&2
		exit 1
	fi
	runtime_file="$(realpath -e "${runtime_file}")"
	if [[ "${runtime_file}" != "${INFRA_ROOT}"/"${DEPLOYMENT_NAME}"/* ]]; then
		printf 'Runtime env symlink crosses the %s environment boundary: %s\n' \
			"${DEPLOYMENT_NAME}" "${runtime_setting}" >&2
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
	MONKEYBOT_BOT_SERVICE_TOKEN

require_database_identity() {
	local file=$1
	local key=$2
	local expected_user=$3
	local expected_database=$4
	local raw_url without_scheme authority database_and_query user_info username database
	raw_url="$(env_value "${file}" "${key}")"
	case "${raw_url}" in
		postgresql://*) without_scheme=${raw_url#postgresql://} ;;
		postgres://*) without_scheme=${raw_url#postgres://} ;;
		*)
			printf 'Database URL has an unsupported scheme: %s in %s\n' "${key}" "${file}" >&2
			exit 1
			;;
	esac
	authority=${without_scheme%%/*}
	database_and_query=${without_scheme#*/}
	if [[ "${authority}" != *@* || "${database_and_query}" == "${without_scheme}" ]]; then
		printf 'Database URL must contain an explicit deployment role and database: %s in %s\n' \
			"${key}" "${file}" >&2
		exit 1
	fi
	user_info=${authority%@*}
	username=${user_info%%:*}
	database=${database_and_query%%\?*}
	database=${database%%#*}
	if [[ "${username}" != "${expected_user}" || "${database}" != "${expected_database}" ]]; then
		printf 'Database URL does not target the expected %s role/database: %s in %s\n' \
			"${DEPLOYMENT_NAME}" "${key}" "${file}" >&2
		exit 1
	fi
}

require_database_identity "${api_runtime_file}" MONKEYBOT_DATABASE_URL \
	"monkeybot_runtime_${DEPLOYMENT_NAME}" "monkeybot_${DEPLOYMENT_NAME}"
require_database_identity "${migration_runtime_file}" MONKEYBOT_MIGRATION_DATABASE_URL \
	"monkeybot_migrator_${DEPLOYMENT_NAME}" "monkeybot_${DEPLOYMENT_NAME}"

reject_placeholder_values() {
	local file=$1
	shift
	for key in "$@"; do
		local value
		value="$(env_value "${file}" "${key}")"
		if [[ "${value}" == *replace-with-* || "${value}" =~ ^0+$ ]]; then
			printf 'Runtime setting still contains an example value: %s in %s\n' "${key}" "${file}" >&2
			exit 1
		fi
	done
}

reject_placeholder_values "${api_runtime_file}" \
	MONKEYBOT_DATABASE_URL \
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
	MONKEYBOT_BOT_SERVICE_TOKEN

if [[ ! "${MONKEYBOT_DISCORD_APPLICATION_ID}" =~ ^[1-9][0-9]{9,19}$ ]]; then
	printf '%s\n' 'MONKEYBOT_DISCORD_APPLICATION_ID must be a Discord application snowflake' >&2
	exit 2
fi
if [[ ! "${MONKEYBOT_BOT_INSTANCE_ID}" =~ ^monkeybot-${DEPLOYMENT_NAME}-[A-Za-z0-9._:-]+$ ]]; then
	printf 'MONKEYBOT_BOT_INSTANCE_ID must identify the %s deployment\n' "${DEPLOYMENT_NAME}" >&2
	exit 2
fi

api_legacy_application_id="$(env_value "${api_runtime_file}" MONKEYBOT_DISCORD_CLIENT_ID)"
if [[ -n "${api_legacy_application_id}" && "${api_legacy_application_id}" != "${MONKEYBOT_DISCORD_APPLICATION_ID}" ]]; then
	printf '%s\n' 'Axum legacy Discord client ID does not match the deployment application ID' >&2
	exit 1
fi

bot_legacy_application_id="$(env_value "${bot_runtime_file}" MONKEYBOT_BOT_APPLICATION_ID)"
if [[ -n "${bot_legacy_application_id}" && "${bot_legacy_application_id}" != "${MONKEYBOT_DISCORD_APPLICATION_ID}" ]]; then
	printf '%s\n' 'Bot legacy Discord application ID does not match the deployment application ID' >&2
	exit 1
fi

bot_runtime_dev_guild_id="$(env_value "${bot_runtime_file}" MONKEYBOT_BOT_DEV_GUILD_ID)"
if [[ -n "${bot_runtime_dev_guild_id}" && "${bot_runtime_dev_guild_id}" != "${MONKEYBOT_BOT_DEV_GUILD_ID:-}" ]]; then
	printf '%s\n' 'Bot legacy development guild ID does not match the deployment guild ID' >&2
	exit 1
fi
bot_runtime_instance_id="$(env_value "${bot_runtime_file}" MONKEYBOT_BOT_INSTANCE_ID)"
if [[ -n "${bot_runtime_instance_id}" && "${bot_runtime_instance_id}" != "${MONKEYBOT_BOT_INSTANCE_ID}" ]]; then
	printf '%s\n' 'Bot legacy instance ID does not match the deployment instance ID' >&2
	exit 1
fi

case "${DEPLOYMENT_NAME}" in
	dev)
		if [[ "${MONKEYBOT_PUBLIC_ORIGIN}" != "https://dev.monkeybot.lol" ]]; then
			printf '%s\n' 'Dev deployment origin must be https://dev.monkeybot.lol' >&2
			exit 2
		fi
		if [[ "${MONKEYBOT_BOT_COMMAND_SCOPE}" != "guild" ]]; then
			printf '%s\n' 'Dev deployment requires MONKEYBOT_BOT_COMMAND_SCOPE=guild' >&2
			exit 2
		fi
		if [[ ! "${MONKEYBOT_BOT_DEV_GUILD_ID:-}" =~ ^[1-9][0-9]{9,19}$ ]]; then
			printf '%s\n' 'Dev deployment requires a Discord guild snowflake in MONKEYBOT_BOT_DEV_GUILD_ID' >&2
			exit 2
		fi
		;;
	main)
		if [[ "${MONKEYBOT_PUBLIC_ORIGIN}" != "https://monkeybot.lol" ]]; then
			printf '%s\n' 'Main deployment origin must be https://monkeybot.lol' >&2
			exit 2
		fi
		if [[ "${MONKEYBOT_BOT_COMMAND_SCOPE}" != "global" ]]; then
			printf '%s\n' 'Main deployment requires MONKEYBOT_BOT_COMMAND_SCOPE=global' >&2
			exit 2
		fi
		if [[ -n "${MONKEYBOT_BOT_DEV_GUILD_ID:-}" ]]; then
			printf '%s\n' 'Main deployment forbids MONKEYBOT_BOT_DEV_GUILD_ID' >&2
			exit 2
		fi
		;;
esac

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
	docker manifest inspect "${MONKEYBOT_WEB_IMAGE}:$(component_image_tag web)" >/dev/null
	docker manifest inspect "${MONKEYBOT_API_IMAGE}:$(component_image_tag api)" >/dev/null
	docker manifest inspect "${MONKEYBOT_RIOT_API_IMAGE}:$(component_image_tag riot-api)" >/dev/null
	docker manifest inspect "${MONKEYBOT_BOT_IMAGE}:$(component_image_tag bot)" >/dev/null
fi

printf 'MonkeyBot %s preflight passed for release %s\n' "${DEPLOYMENT_NAME}" "${MONKEYBOT_RELEASE_TAG}"
