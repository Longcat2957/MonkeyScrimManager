#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
common_file=${MONKEYBOT_LOCAL_DISCORD_ENV:-${root_dir}/.local/env/dev/discord.env}
api_file=${MONKEYBOT_LOCAL_API_ENV:-${root_dir}/.local/env/dev/api.env}
bot_file=${MONKEYBOT_LOCAL_BOT_ENV:-${root_dir}/.local/env/dev/bot.env}

for file in "${common_file}" "${api_file}" "${bot_file}"; do
	if [[ ! -f "${file}" ]]; then
		printf 'Required local runtime file does not exist: %s\n' "${file}" >&2
		exit 1
	fi
	mode=$(stat -c '%a' "${file}")
	if [[ "${mode}" != "600" ]]; then
		printf 'Local runtime file must have mode 600: %s (got %s)\n' "${file}" "${mode}" >&2
		exit 1
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
			if (name != target) next
			value = substr($0, separator + 1)
			sub(/\r$/, "", value)
			print value
			exit
		}
	' "${file}"
}

require_value() {
	local file=$1
	local key=$2
	local value
	value=$(env_value "${file}" "${key}")
	if [[ -z "${value}" || "${value}" == *replace-with-* || "${value}" =~ ^0+$ ]]; then
		printf 'Required local setting is missing or still a placeholder: %s in %s\n' "${key}" "${file}" >&2
		exit 1
	fi
}

for key in \
	MONKEYBOT_DISCORD_APPLICATION_ID \
	MONKEYBOT_BOT_COMMAND_SCOPE \
	MONKEYBOT_BOT_DEV_GUILD_ID; do
	require_value "${common_file}" "${key}"
done
for key in \
	MONKEYBOT_DATABASE_URL \
	MONKEYBOT_DISCORD_CLIENT_SECRET \
	MONKEYBOT_OAUTH_ENCRYPTION_KEY \
	MONKEYBOT_SERVICE_ADMIN_IDS \
	MONKEYBOT_BOT_SERVICE_TOKEN; do
	require_value "${api_file}" "${key}"
done
for key in \
	MONKEYBOT_BOT_TOKEN \
	MONKEYBOT_BOT_API_BASE_URL \
	MONKEYBOT_BOT_SERVICE_TOKEN \
	MONKEYBOT_BOT_INSTANCE_ID \
	MONKEYBOT_WEB_ORIGIN; do
	require_value "${bot_file}" "${key}"
done

application_id=$(env_value "${common_file}" MONKEYBOT_DISCORD_APPLICATION_ID)
dev_guild_id=$(env_value "${common_file}" MONKEYBOT_BOT_DEV_GUILD_ID)
command_scope=$(env_value "${common_file}" MONKEYBOT_BOT_COMMAND_SCOPE)
if [[ ! "${application_id}" =~ ^[1-9][0-9]{9,19}$ ]]; then
	printf '%s\n' 'Local Discord application ID must be a Discord snowflake' >&2
	exit 1
fi
if [[ "${command_scope}" != "guild" ]]; then
	printf '%s\n' 'Local Bot command scope must be guild' >&2
	exit 1
fi
if [[ ! "${dev_guild_id}" =~ ^[1-9][0-9]{9,19}$ ]]; then
	printf '%s\n' 'Local development guild ID must be a Discord snowflake' >&2
	exit 1
fi

api_legacy_id=$(env_value "${api_file}" MONKEYBOT_DISCORD_CLIENT_ID)
bot_legacy_id=$(env_value "${bot_file}" MONKEYBOT_BOT_APPLICATION_ID)
bot_legacy_guild_id=$(env_value "${bot_file}" MONKEYBOT_BOT_DEV_GUILD_ID)
if [[ -n "${api_legacy_id}" && "${api_legacy_id}" != "${application_id}" ]]; then
	printf '%s\n' 'Local API legacy Discord client ID does not match discord.env' >&2
	exit 1
fi
if [[ -n "${bot_legacy_id}" && "${bot_legacy_id}" != "${application_id}" ]]; then
	printf '%s\n' 'Local Bot legacy Discord application ID does not match discord.env' >&2
	exit 1
fi
if [[ -n "${bot_legacy_guild_id}" && "${bot_legacy_guild_id}" != "${dev_guild_id}" ]]; then
	printf '%s\n' 'Local Bot legacy development guild ID does not match discord.env' >&2
	exit 1
fi

api_service_token=$(env_value "${api_file}" MONKEYBOT_BOT_SERVICE_TOKEN)
bot_service_token=$(env_value "${bot_file}" MONKEYBOT_BOT_SERVICE_TOKEN)
if [[ "${api_service_token}" != "${bot_service_token}" ]]; then
	printf '%s\n' 'Local API and Bot service tokens do not match' >&2
	exit 1
fi

printf '%s\n' 'Local Discord environment preflight passed'
