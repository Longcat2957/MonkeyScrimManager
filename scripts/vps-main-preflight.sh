#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

export MONKEYBOT_DEV_COMPOSE_FILE="${MONKEYBOT_MAIN_COMPOSE_FILE:-${ROOT_DIR}/deploy/docker-compose.main.yml}"
export MONKEYBOT_DEV_ENV_FILE="${MONKEYBOT_MAIN_ENV_FILE:-${ROOT_DIR}/deploy/main.env}"
export MONKEYBOT_DEPLOYMENT_NAME=main

exec "${ROOT_DIR}/scripts/vps-dev-preflight.sh"
