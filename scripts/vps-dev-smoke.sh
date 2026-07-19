#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${MONKEYBOT_DEV_COMPOSE_FILE:-${ROOT_DIR}/deploy/docker-compose.dev.yml}"
ENV_FILE="${MONKEYBOT_DEV_ENV_FILE:-${ROOT_DIR}/deploy/dev.env}"
RIOT_SERVICE="${MONKEYBOT_RIOT_SERVICE_NAME:-riot-api-dev}"

if [[ ! -f "${ENV_FILE}" ]]; then
	printf 'Deployment env file does not exist: %s\n' "${ENV_FILE}" >&2
	exit 2
fi

set -a
# shellcheck disable=SC1090
. "${ENV_FILE}"
set +a

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

api_origin="http://127.0.0.1:${MONKEYBOT_API_PORT}"
bot_origin="http://127.0.0.1:${MONKEYBOT_BOT_HEALTH_PORT}"
public_origin="${MONKEYBOT_PUBLIC_ORIGIN%/}"
riot_service_token="$(env_value "${MONKEYBOT_API_RUNTIME_ENV:?set MONKEYBOT_API_RUNTIME_ENV}" MONKEYBOT_RIOT_SERVICE_TOKEN)"

if [[ -z "${riot_service_token}" ]]; then
	printf '%s\n' 'MONKEYBOT_RIOT_SERVICE_TOKEN is missing from the API runtime file' >&2
	exit 1
fi

expect_status() {
	local label=$1
	local expected=$2
	local url=$3
	local status
	status="$(curl --silent --show-error --max-time 10 --output /dev/null --write-out '%{http_code}' "${url}" || true)"
	if [[ "${status}" != "${expected}" ]]; then
		printf '%s expected HTTP %s, got %s (%s)\n' "${label}" "${expected}" "${status}" "${url}" >&2
		exit 1
	fi
	printf '%s HTTP %s\n' "${label}" "${status}"
}

expect_route_status() {
	local label=$1
	local url=$2
	local status
	status="$(curl --silent --show-error --http1.1 --max-time 10 --output /dev/null --write-out '%{http_code}' \
		-H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
		-H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' "${url}" || true)"
	case "${status}" in
		101|401|403|426)
			printf '%s HTTP %s\n' "${label}" "${status}"
			;;
		*)
			printf '%s reached an invalid route status %s (%s)\n' "${label}" "${status}" "${url}" >&2
			exit 1
			;;
	esac
}

expect_status 'API liveness' 200 "${api_origin}/healthz"
expect_status 'API readiness' 200 "${api_origin}/readyz"
expect_status 'Bot health' 200 "${bot_origin}/healthz"
expect_status 'Public health' 200 "${public_origin}/healthz"
expect_status 'Public landing' 200 "${public_origin}/landing"
expect_status 'Public install' 200 "${public_origin}/install"
expect_status 'Unauthenticated API contract' 401 "${public_origin}/api/me"
expect_route_status 'WebSocket route' "${public_origin}/ws"

docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" exec -T "${RIOT_SERVICE}" \
	curl --fail --silent --show-error http://127.0.0.1:8081/healthz >/dev/null
printf 'Riot integration health HTTP 200\n'

riot_auth_status="$(
	docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" exec -T "${RIOT_SERVICE}" \
		curl --silent --show-error --max-time 10 --output /dev/null --write-out '%{http_code}' \
			-H "Authorization: Bearer ${riot_service_token}" \
			'http://127.0.0.1:8081/internal/riot/records?gameName=&tagLine=KR1&platform=KR' || true
)"
if [[ "${riot_auth_status}" != "400" ]]; then
	printf 'Riot integration internal-auth smoke expected HTTP 400, got %s\n' "${riot_auth_status}" >&2
	exit 1
fi
printf 'Riot integration internal-auth HTTP 400\n'

if [[ -n "${MONKEYBOT_RIOT_SMOKE_GAME_NAME:-}" && -n "${MONKEYBOT_RIOT_SMOKE_TAG_LINE:-}" ]]; then
	riot_smoke_platform="${MONKEYBOT_RIOT_SMOKE_PLATFORM:-KR}"
	riot_smoke_payload="$(
		printf '{"platform":"%s","lookup":{"kind":"riotId","gameName":"%s","tagLine":"%s"}}' \
			"${riot_smoke_platform}" "${MONKEYBOT_RIOT_SMOKE_GAME_NAME}" "${MONKEYBOT_RIOT_SMOKE_TAG_LINE}"
	)"
	docker compose --env-file "${ENV_FILE}" -f "${COMPOSE_FILE}" exec -T "${RIOT_SERVICE}" \
		curl --fail --silent --show-error --max-time 20 \
			-H "Authorization: Bearer ${riot_service_token}" \
			-H 'Content-Type: application/json' \
			--data "${riot_smoke_payload}" \
			http://127.0.0.1:8081/internal/riot/accounts/resolve >/dev/null
	printf 'Riot provider account resolve HTTP 200\n'
fi

printf '%s\n' 'Manual gate remaining: complete one Discord OAuth callback in the browser.'
