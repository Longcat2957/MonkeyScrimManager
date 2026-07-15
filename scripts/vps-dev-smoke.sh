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

api_origin="http://127.0.0.1:${MONKEYBOT_API_PORT}"
bot_origin="http://127.0.0.1:${MONKEYBOT_BOT_HEALTH_PORT}"
public_origin="${MONKEYBOT_PUBLIC_ORIGIN%/}"

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

printf '%s\n' 'Manual gate remaining: complete one Discord OAuth callback in the browser.'
