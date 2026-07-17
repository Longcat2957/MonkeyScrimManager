#!/usr/bin/env bash
set -euo pipefail

dev_env_file=${MONKEYBOT_DEV_ENV_FILE:-/root/monkeybot-dev/deploy/dev.env}
main_env_file=${MONKEYBOT_MAIN_ENV_FILE:-/root/monkeybot-main/deploy/main.env}
infra_root=${MONKEYBOT_INFRA_ROOT:-/root/monkeybot-infra}
infra_root=${infra_root%/}

for file in "${dev_env_file}" "${main_env_file}"; do
	if [[ ! -f "${file}" ]]; then
		printf 'Deployment env file does not exist: %s\n' "${file}" >&2
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

require_value() {
	local file=$1
	local key=$2
	local value
	value=$(env_value "${file}" "${key}")
	if [[ -z "${value}" || "${value}" == *replace-with-* || "${value}" =~ ^0+$ ]]; then
		printf 'Required isolation-audit setting is missing or still a placeholder: %s in %s\n' \
			"${key}" "${file}" >&2
		exit 1
	fi
	printf '%s\n' "${value}"
}

runtime_file() {
	local deployment=$1
	local env_file=$2
	local key=$3
	local configured resolved expected_root
	configured=$(require_value "${env_file}" "${key}")
	expected_root=${infra_root}/${deployment}
	if [[ "${configured}" != "${expected_root}"/* || ! -f "${configured}" ]]; then
		printf 'Runtime file is outside the %s isolation root or missing: %s\n' \
			"${deployment}" "${key}" >&2
		exit 1
	fi
	resolved=$(realpath -e "${configured}")
	expected_root=$(realpath -e "${expected_root}")
	if [[ "${resolved}" != "${expected_root}"/* ]]; then
		printf 'Runtime file symlink crosses the %s isolation root: %s\n' \
			"${deployment}" "${key}" >&2
		exit 1
	fi
	if [[ $(stat -c '%a' "${resolved}") != 600 ]]; then
		printf 'Runtime file must have mode 600 for isolation audit: %s\n' "${key}" >&2
		exit 1
	fi
	printf '%s\n' "${resolved}"
}

assert_same_secret() {
	local label=$1
	local left_file=$2
	local left_key=$3
	local right_file=$4
	local right_key=$5
	local left right
	left=$(require_value "${left_file}" "${left_key}")
	right=$(require_value "${right_file}" "${right_key}")
	if [[ "${left}" != "${right}" ]]; then
		printf '%s does not match inside one deployment\n' "${label}" >&2
		exit 1
	fi
}

assert_distinct_secret() {
	local label=$1
	local dev_file=$2
	local dev_key=$3
	local main_file=$4
	local main_key=$5
	local dev_value main_value
	dev_value=$(require_value "${dev_file}" "${dev_key}")
	main_value=$(require_value "${main_file}" "${main_key}")
	if [[ "${dev_value}" == "${main_value}" ]]; then
		printf 'Dev and main must not share %s\n' "${label}" >&2
		exit 1
	fi
}

dev_application_id=$(require_value "${dev_env_file}" MONKEYBOT_DISCORD_APPLICATION_ID)
main_application_id=$(require_value "${main_env_file}" MONKEYBOT_DISCORD_APPLICATION_ID)
if [[ ! "${dev_application_id}" =~ ^[1-9][0-9]{9,19}$ || ! "${main_application_id}" =~ ^[1-9][0-9]{9,19}$ ]]; then
	printf '%s\n' 'Dev and main Discord application IDs must be Discord snowflakes' >&2
	exit 1
fi
if [[ "${dev_application_id}" == "${main_application_id}" ]]; then
	printf '%s\n' 'Dev and main must use different Discord application IDs' >&2
	exit 1
fi
dev_origin=$(require_value "${dev_env_file}" MONKEYBOT_PUBLIC_ORIGIN)
main_origin=$(require_value "${main_env_file}" MONKEYBOT_PUBLIC_ORIGIN)
if [[ "${dev_origin}" != https://dev.monkeybot.lol || "${main_origin}" != https://monkeybot.lol ]]; then
	printf '%s\n' 'Dev or main public origin violates the isolation contract' >&2
	exit 1
fi
dev_scope=$(require_value "${dev_env_file}" MONKEYBOT_BOT_COMMAND_SCOPE)
main_scope=$(require_value "${main_env_file}" MONKEYBOT_BOT_COMMAND_SCOPE)
if [[ "${dev_scope}" != guild || "${main_scope}" != global ]]; then
	printf '%s\n' 'Dev or main command scope violates the isolation contract' >&2
	exit 1
fi
dev_guild_id=$(require_value "${dev_env_file}" MONKEYBOT_BOT_DEV_GUILD_ID)
main_guild_id=$(env_value "${main_env_file}" MONKEYBOT_BOT_DEV_GUILD_ID)
if [[ ! "${dev_guild_id}" =~ ^[1-9][0-9]{9,19}$ || -n "${main_guild_id}" ]]; then
	printf '%s\n' 'Development guild ID must exist only in the dev deployment' >&2
	exit 1
fi
dev_instance_id=$(require_value "${dev_env_file}" MONKEYBOT_BOT_INSTANCE_ID)
main_instance_id=$(require_value "${main_env_file}" MONKEYBOT_BOT_INSTANCE_ID)
if [[ ! "${dev_instance_id}" =~ ^monkeybot-dev-[A-Za-z0-9._:-]+$ || \
	! "${main_instance_id}" =~ ^monkeybot-main-[A-Za-z0-9._:-]+$ ]]; then
	printf '%s\n' 'Bot instance IDs must identify their deployment' >&2
	exit 1
fi

dev_api_runtime=$(runtime_file dev "${dev_env_file}" MONKEYBOT_API_RUNTIME_ENV)
dev_migration_runtime=$(runtime_file dev "${dev_env_file}" MONKEYBOT_MIGRATION_RUNTIME_ENV)
dev_riot_runtime=$(runtime_file dev "${dev_env_file}" MONKEYBOT_RIOT_RUNTIME_ENV)
dev_bot_runtime=$(runtime_file dev "${dev_env_file}" MONKEYBOT_BOT_RUNTIME_ENV)
main_api_runtime=$(runtime_file main "${main_env_file}" MONKEYBOT_API_RUNTIME_ENV)
main_migration_runtime=$(runtime_file main "${main_env_file}" MONKEYBOT_MIGRATION_RUNTIME_ENV)
main_riot_runtime=$(runtime_file main "${main_env_file}" MONKEYBOT_RIOT_RUNTIME_ENV)
main_bot_runtime=$(runtime_file main "${main_env_file}" MONKEYBOT_BOT_RUNTIME_ENV)

runtime_files=(
	"${dev_api_runtime}"
	"${dev_migration_runtime}"
	"${dev_riot_runtime}"
	"${dev_bot_runtime}"
	"${main_api_runtime}"
	"${main_migration_runtime}"
	"${main_riot_runtime}"
	"${main_bot_runtime}"
)
for ((left_index = 0; left_index < ${#runtime_files[@]}; left_index++)); do
	for ((right_index = left_index + 1; right_index < ${#runtime_files[@]}; right_index++)); do
		if [[ "${runtime_files[left_index]}" == "${runtime_files[right_index]}" ]]; then
			printf '%s\n' 'A runtime file is reused across process or deployment boundaries' >&2
			exit 1
		fi
	done
done

assert_same_secret 'Dev Bot service token' \
	"${dev_api_runtime}" MONKEYBOT_BOT_SERVICE_TOKEN \
	"${dev_bot_runtime}" MONKEYBOT_BOT_SERVICE_TOKEN
assert_same_secret 'Main Bot service token' \
	"${main_api_runtime}" MONKEYBOT_BOT_SERVICE_TOKEN \
	"${main_bot_runtime}" MONKEYBOT_BOT_SERVICE_TOKEN
assert_same_secret 'Dev Riot service token' \
	"${dev_api_runtime}" MONKEYBOT_RIOT_SERVICE_TOKEN \
	"${dev_riot_runtime}" MONKEYBOT_RIOT_INTERNAL_TOKEN
assert_same_secret 'Main Riot service token' \
	"${main_api_runtime}" MONKEYBOT_RIOT_SERVICE_TOKEN \
	"${main_riot_runtime}" MONKEYBOT_RIOT_INTERNAL_TOKEN

assert_distinct_secret 'Discord OAuth client secrets' \
	"${dev_api_runtime}" MONKEYBOT_DISCORD_CLIENT_SECRET \
	"${main_api_runtime}" MONKEYBOT_DISCORD_CLIENT_SECRET
assert_distinct_secret 'OAuth encryption keys' \
	"${dev_api_runtime}" MONKEYBOT_OAUTH_ENCRYPTION_KEY \
	"${main_api_runtime}" MONKEYBOT_OAUTH_ENCRYPTION_KEY
assert_distinct_secret 'Discord Bot tokens' \
	"${dev_bot_runtime}" MONKEYBOT_BOT_TOKEN \
	"${main_bot_runtime}" MONKEYBOT_BOT_TOKEN
assert_distinct_secret 'Bot service tokens' \
	"${dev_api_runtime}" MONKEYBOT_BOT_SERVICE_TOKEN \
	"${main_api_runtime}" MONKEYBOT_BOT_SERVICE_TOKEN
assert_distinct_secret 'Riot service tokens' \
	"${dev_api_runtime}" MONKEYBOT_RIOT_SERVICE_TOKEN \
	"${main_api_runtime}" MONKEYBOT_RIOT_SERVICE_TOKEN
assert_distinct_secret 'runtime database URLs' \
	"${dev_api_runtime}" MONKEYBOT_DATABASE_URL \
	"${main_api_runtime}" MONKEYBOT_DATABASE_URL
assert_distinct_secret 'migration database URLs' \
	"${dev_migration_runtime}" MONKEYBOT_MIGRATION_DATABASE_URL \
	"${main_migration_runtime}" MONKEYBOT_MIGRATION_DATABASE_URL

printf '%s\n' 'MonkeyBot dev/main environment isolation audit passed'
