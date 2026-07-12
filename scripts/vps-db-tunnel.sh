#!/usr/bin/env bash
set -euo pipefail

vps_host="${MONKEYBOT_VPS_HOST:-root@141.164.46.191}"
local_port="${MONKEYBOT_DB_TUNNEL_LOCAL_PORT:-15432}"
remote_port="${MONKEYBOT_DB_TUNNEL_REMOTE_PORT:-10010}"

if ! [[ "${local_port}" =~ ^[0-9]+$ ]] || ((local_port < 1 || local_port > 65535)); then
	printf 'invalid local tunnel port: %s\n' "${local_port}" >&2
	exit 2
fi

if ! [[ "${remote_port}" =~ ^[0-9]+$ ]] || ((remote_port < 1 || remote_port > 65535)); then
	printf 'invalid remote PostgreSQL port: %s\n' "${remote_port}" >&2
	exit 2
fi

if ss -lnt "sport = :${local_port}" | tail -n +2 | grep -q .; then
	printf 'local port %s is already in use\n' "${local_port}" >&2
	exit 1
fi

printf 'Forwarding 127.0.0.1:%s -> %s:127.0.0.1:%s\n' \
	"${local_port}" "${vps_host}" "${remote_port}"

exec ssh \
	-o BatchMode=yes \
	-o ExitOnForwardFailure=yes \
	-o ServerAliveInterval=30 \
	-o ServerAliveCountMax=3 \
	-N \
	-L "127.0.0.1:${local_port}:127.0.0.1:${remote_port}" \
	"${vps_host}"
