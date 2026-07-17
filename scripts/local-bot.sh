#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
common_file=${MONKEYBOT_LOCAL_DISCORD_ENV:-${root_dir}/.local/env/dev/discord.env}
bot_file=${MONKEYBOT_LOCAL_BOT_ENV:-${root_dir}/.local/env/dev/bot.env}
action=${1:-start}

"${root_dir}/scripts/local-discord-preflight.sh"
set -a
# shellcheck disable=SC1090
. "${common_file}"
# shellcheck disable=SC1090
. "${bot_file}"
set +a

cd "${root_dir}"
case "${action}" in
	start) exec pnpm --filter @monkeybot/bot dev ;;
	register-guild) exec pnpm --filter @monkeybot/bot commands:register:guild ;;
	clear-legacy-global) exec pnpm --filter @monkeybot/bot commands:clear:legacy-global ;;
	*)
		printf 'Usage: %s [start|register-guild|clear-legacy-global]\n' "$0" >&2
		exit 2
		;;
esac
