#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
service_name=${MONKEYBOT_LOCAL_API_SERVICE:-monkeybot-api-local.service}
api_origin=${MONKEYBOT_LOCAL_API_ORIGIN:-http://127.0.0.1:30020}
web_origin=${MONKEYBOT_LOCAL_WEB_ORIGIN:-http://127.0.0.1:5173}
smoke_path=${1:-/api/me}

case "$smoke_path" in
	/api/*) ;;
	*)
		printf 'Smoke path must start with /api/: %s\n' "$smoke_path" >&2
		exit 2
		;;
esac

cd "$repo_root"
cargo build --locked --package monkeybot-api --bin monkeybot-api

if ! systemctl --user cat "$service_name" >/dev/null 2>&1; then
	printf 'Local API service does not exist: %s\n' "$service_name" >&2
	exit 1
fi

systemctl --user restart "$service_name"

ready_status=000
for _ in $(seq 1 30); do
	ready_status=$(curl --silent --show-error --max-time 5 --output /dev/null \
		--write-out '%{http_code}' "$api_origin/readyz" || true)
	if [[ "$ready_status" == 200 ]]; then
		break
	fi
	sleep 1
done

if [[ "$ready_status" != 200 ]]; then
	printf 'Local API did not become ready: HTTP %s\n' "$ready_status" >&2
	exit 1
fi

smoke() {
	local label=$1
	local origin=$2
	local status
	status=$(curl --silent --show-error --max-time 10 --output /dev/null \
		--write-out '%{http_code}' "$origin$smoke_path" || true)
	case "$status" in
		000|404|5??)
			printf '%s route smoke failed: HTTP %s (%s%s)\n' \
				"$label" "$status" "$origin" "$smoke_path" >&2
			exit 1
			;;
		*) printf '%s route smoke passed: HTTP %s\n' "$label" "$status" ;;
	esac
}

smoke direct "$api_origin"
smoke proxy "$web_origin"
printf 'Local API reload completed: %s\n' "$smoke_path"
