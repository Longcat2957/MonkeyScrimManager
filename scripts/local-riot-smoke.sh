#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
API_ENV="${MONKEYBOT_LOCAL_API_ENV:-${ROOT_DIR}/.local/env/dev/api.env}"
RIOT_ENV="${MONKEYBOT_LOCAL_RIOT_ENV:-${ROOT_DIR}/.local/env/dev/riot-api.env}"

for file in "${API_ENV}" "${RIOT_ENV}"; do
	if [[ ! -f "${file}" ]]; then
		printf 'Runtime env file does not exist: %s\n' "${file}" >&2
		exit 2
	fi
done

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

api_token="$(env_value "${API_ENV}" MONKEYBOT_RIOT_SERVICE_TOKEN)"
riot_token="$(env_value "${RIOT_ENV}" MONKEYBOT_RIOT_INTERNAL_TOKEN)"
riot_api_key="$(env_value "${RIOT_ENV}" MONKEYBOT_RIOT_API_KEY)"
bind_addr="$(env_value "${RIOT_ENV}" MONKEYBOT_RIOT_BIND_ADDR)"
service_url="$(env_value "${API_ENV}" MONKEYBOT_RIOT_SERVICE_URL)"

if [[ -z "${api_token}" || -z "${riot_token}" || -z "${riot_api_key}" || -z "${bind_addr}" || -z "${service_url}" ]]; then
	printf '%s\n' 'Riot local runtime settings are incomplete' >&2
	exit 1
fi
case "${api_token}:${riot_token}:${riot_api_key}" in
	*replace-with-*)
		printf '%s\n' 'Riot local runtime settings still contain example placeholder values' >&2
		exit 1
		;;
esac
if [[ "${api_token}" != "${riot_token}" ]]; then
	printf '%s\n' 'MONKEYBOT_RIOT_SERVICE_TOKEN and MONKEYBOT_RIOT_INTERNAL_TOKEN do not match' >&2
	exit 1
fi
case "${service_url}" in
	http://*/internal/riot/|https://*/internal/riot/) ;;
	*)
		printf '%s\n' 'MONKEYBOT_RIOT_SERVICE_URL must end with /internal/riot/' >&2
		exit 1
		;;
esac

origin="http://${bind_addr}"
curl --fail --silent --show-error --max-time 5 "${origin}/healthz" >/dev/null
printf '%s\n' 'Riot adapter health HTTP 200'

unauthorized_status="$(curl --silent --show-error --max-time 5 \
	--output /dev/null --write-out '%{http_code}' \
	"${origin}/internal/riot/records?gameName=&tagLine=KR1&platform=KR" || true)"
if [[ "${unauthorized_status}" != "401" ]]; then
	printf 'Riot adapter unauthenticated route expected HTTP 401, got %s\n' "${unauthorized_status}" >&2
	exit 1
fi
printf '%s\n' 'Riot adapter rejects missing internal auth HTTP 401'

authorized_invalid_status="$(curl --silent --show-error --max-time 5 \
	--output /dev/null --write-out '%{http_code}' \
	-H "Authorization: Bearer ${api_token}" \
	"${origin}/internal/riot/records?gameName=&tagLine=KR1&platform=KR" || true)"
if [[ "${authorized_invalid_status}" != "400" ]]; then
	printf 'Riot adapter authenticated invalid-input route expected HTTP 400, got %s\n' \
		"${authorized_invalid_status}" >&2
	exit 1
fi
printf '%s\n' 'Riot adapter accepts matching internal auth HTTP 400'

if [[ -n "${MONKEYBOT_RIOT_SMOKE_GAME_NAME:-}" && -n "${MONKEYBOT_RIOT_SMOKE_TAG_LINE:-}" ]]; then
	platform="${MONKEYBOT_RIOT_SMOKE_PLATFORM:-KR}"
	payload="$(
		printf '{"platform":"%s","lookup":{"kind":"riotId","gameName":"%s","tagLine":"%s"}}' \
			"${platform}" "${MONKEYBOT_RIOT_SMOKE_GAME_NAME}" "${MONKEYBOT_RIOT_SMOKE_TAG_LINE}"
	)"
	curl --fail --silent --show-error --max-time 20 \
		-H "Authorization: Bearer ${api_token}" \
		-H 'Content-Type: application/json' \
		--data "${payload}" \
		"${origin}/internal/riot/accounts/resolve" >/dev/null
	printf '%s\n' 'Riot provider account resolve HTTP 200'
else
	printf '%s\n' 'Skipped live Riot provider lookup; set MONKEYBOT_RIOT_SMOKE_GAME_NAME and MONKEYBOT_RIOT_SMOKE_TAG_LINE to enable it.'
fi
