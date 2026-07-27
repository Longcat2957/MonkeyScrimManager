#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${MONKEYBOT_MAIN_COMPOSE_FILE:-${ROOT_DIR}/deploy/docker-compose.main.yml}"
ENV_FILE="${MONKEYBOT_MAIN_ENV_FILE:-${ROOT_DIR}/deploy/main.env}"
WAIT_TIMEOUT="${MONKEYBOT_COMPONENT_ROLLOUT_WAIT_TIMEOUT:-90}"

usage() {
	printf '%s\n' \
		'Usage: scripts/vps-main-component-rollout.sh <web|api|riot-api|bot> <image-tag> --compatible' \
		'Use only for a migration-free, backward-compatible component hotfix.' >&2
}

if [[ "$#" -ne 3 || "${3}" != "--compatible" ]]; then
	usage
	exit 2
fi

component=$1
image_tag=$2
case "${component}" in
	web)
		service=web-main
		tag_setting=MONKEYBOT_WEB_IMAGE_TAG
		;;
	api)
		service=api-main
		tag_setting=MONKEYBOT_API_IMAGE_TAG
		;;
	riot-api)
		service=riot-api-main
		tag_setting=MONKEYBOT_RIOT_API_IMAGE_TAG
		;;
	bot)
		service=bot-main
		tag_setting=MONKEYBOT_BOT_IMAGE_TAG
		;;
	*)
	usage
	exit 2
	;;
esac

if [[ ! "${image_tag}" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]]; then
	printf 'Invalid image tag: %s\n' "${image_tag}" >&2
	exit 2
fi
if [[ ! "${WAIT_TIMEOUT}" =~ ^[1-9][0-9]*$ ]]; then
	printf 'MONKEYBOT_COMPONENT_ROLLOUT_WAIT_TIMEOUT must be a positive integer\n' >&2
	exit 2
fi
if [[ ! -f "${COMPOSE_FILE}" || ! -f "${ENV_FILE}" ]]; then
	printf 'Main deployment files are missing\n' >&2
	exit 2
fi

set -a
# shellcheck disable=SC1090
. "${ENV_FILE}"
set +a
old_release_tag="${MONKEYBOT_RELEASE_TAG:?MONKEYBOT_RELEASE_TAG must be set}"

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

backup_path="$(mktemp /tmp/monkeybot-main.env.component-rollout.XXXXXX)"
cp -p "${ENV_FILE}" "${backup_path}"
working_env="$(mktemp "${ENV_FILE}.XXXXXX")"
cp -p "${ENV_FILE}" "${working_env}"

# Materialize every component tag before moving the release marker so unchanged services
# continue to resolve to the exact image they are already running.
for setting in \
	MONKEYBOT_WEB_IMAGE_TAG \
	MONKEYBOT_API_IMAGE_TAG \
	MONKEYBOT_RIOT_API_IMAGE_TAG \
	MONKEYBOT_BOT_IMAGE_TAG; do
	current_tag="${!setting-}"
	write_env_value "${working_env}" "${setting}" "${current_tag:-${old_release_tag}}"
done
write_env_value "${working_env}" "${tag_setting}" "${image_tag}"
write_env_value "${working_env}" MONKEYBOT_RELEASE_TAG "${image_tag}"
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

if ! docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" pull "${service}"; then
	cp -p "${backup_path}" "${ENV_FILE}"
	printf 'Image pull failed; restored deployment env from %s\n' "${backup_path}" >&2
	exit 1
fi
if ! docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" up -d --no-deps --wait \
	--wait-timeout "${WAIT_TIMEOUT}" "${service}"; then
	printf 'Component recreate failed; rollback env backup: %s\n' "${backup_path}" >&2
	exit 1
fi

case "${component}" in
	web|api)
		docker exec nginx nginx -t
		docker exec nginx nginx -s reload
		;;
esac

MONKEYBOT_MAIN_COMPOSE_FILE="${COMPOSE_FILE}" \
	MONKEYBOT_MAIN_ENV_FILE="${ENV_FILE}" \
	"${ROOT_DIR}/scripts/vps-main-smoke.sh"

printf 'Component rollout completed: %s (%s)\n' "${component}" "${image_tag}"
printf 'Rollback env backup: %s\n' "${backup_path}"
