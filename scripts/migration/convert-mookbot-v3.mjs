#!/usr/bin/env node

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

process.umask(0o077);

const TABLE_NAMES = [
	'users',
	'user_discord_identities',
	'user_privacy_settings',
	'user_champion_preferences',
	'riot_accounts',
	'dummy_user_profiles',
	'guild_members',
	'user_guild_preferences',
	'guild_counters',
	'seasons',
	'recruitments',
	'recruitment_participants',
	'recruitment_preferred_lanes',
	'recruitment_entry_drafts',
	'recruitment_entry_assignments',
	'series',
	'series_participants',
	'auction_recruitments',
	'auction_recruitment_participants',
	'auction_setup_drafts',
	'auction_setup_teams',
	'auction_tournaments',
	'auction_teams',
	'auction_bid_candidates',
	'auction_lots',
	'auction_actions',
	'auction_team_members',
	'auction_point_ledger',
	'auction_bracket_slots',
	'auction_matches',
	'auction_match_lineups',
	'game_inputs',
	'game_input_picks',
	'game_input_bans',
	'games',
	'game_participants',
	'game_participant_stats',
	'game_bans',
	'user_lane_mmr',
	'mmr_ledger_entries',
	'admin_audit_log'
];

const LANES = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];
const TEAMS = ['TEAM_1', 'TEAM_2'];
const DEFAULT_MMR = 1500;
const MAX_ARCHIVE_ROWS = 250_000;
const MAX_ARCHIVE_BYTES = 16 * 1024 * 1024;
const SECRET_KEY = /(authorization|cookie|password|secret|token|api[_-]?key)/i;

function parseArgs(argv) {
	const args = {};
	for (let index = 0; index < argv.length; index += 1) {
		const key = argv[index];
		if (!key.startsWith('--')) throw new Error(`unexpected argument: ${key}`);
		const value = argv[index + 1];
		if (!value || value.startsWith('--')) throw new Error(`missing value for ${key}`);
		args[key.slice(2)] = value;
		index += 1;
	}
	return args;
}

const args = parseArgs(process.argv.slice(2));
for (const key of [
	'snapshot',
	'champions',
	'guild-id',
	'guild-name',
	'write-freeze-at',
	'output',
	'report'
]) {
	if (!args[key]) throw new Error(`--${key} is required`);
}
if (!/^[1-9][0-9]{9,19}$/.test(args['guild-id'])) {
	throw new Error('--guild-id must be a Discord snowflake');
}
if (args['guild-name'].trim().length === 0 || args['guild-name'].trim().length > 100) {
	throw new Error('--guild-name must contain 1 to 100 characters');
}

const snapshotDir = path.resolve(args.snapshot);
const outputPath = path.resolve(args.output);
const reportPath = path.resolve(args.report);
const guildId = args['guild-id'];
const guildName = args['guild-name'].trim();
const writeFreezeAt = new Date(args['write-freeze-at']);
if (Number.isNaN(writeFreezeAt.getTime()))
	throw new Error('--write-freeze-at must be an ISO-8601 timestamp');
const freezeIso = writeFreezeAt.toISOString();
const freezeSeconds = Math.floor(writeFreezeAt.getTime() / 1000);

const manifest = JSON.parse(
	fs.readFileSync(path.join(snapshotDir, 'manifest-preclean.json'), 'utf8')
);
const analysis = JSON.parse(
	fs.readFileSync(path.join(snapshotDir, 'analysis-report.json'), 'utf8')
);
if (manifest.format !== 'MOOKBOT_V3_PRECLEAN_SNAPSHOT' || manifest.schemaVersion !== 1) {
	throw new Error('unsupported source snapshot manifest');
}
if (!manifest.validation?.stableDoubleRead || manifest.validation?.foreignKeyViolations?.length) {
	throw new Error('source snapshot is not stable or has foreign-key violations');
}
if (!analysis.validation?.readyForArchiveConversion || analysis.validation?.errorCount !== 0) {
	throw new Error('source analysis is not ready for archive conversion');
}

function readJsonl(filePath) {
	return fs
		.readFileSync(filePath, 'utf8')
		.split('\n')
		.filter(Boolean)
		.map((line) => JSON.parse(line));
}

function loadTable(name) {
	return readJsonl(path.join(snapshotDir, 'raw-jsonl', `${name}.jsonl`));
}

function groupBy(rows, key) {
	const grouped = new Map();
	for (const row of rows) {
		const value = typeof key === 'function' ? key(row) : row[key];
		const bucket = grouped.get(value) ?? [];
		bucket.push(row);
		grouped.set(value, bucket);
	}
	return grouped;
}

function indexBy(rows, key) {
	return new Map(rows.map((row) => [row[key], row]));
}

function compareText(left, right) {
	return String(left).localeCompare(String(right), 'en');
}

function isoFromUnix(value, fallback = freezeSeconds) {
	const seconds = value == null ? fallback : Number(value);
	if (!Number.isFinite(seconds)) throw new Error(`invalid unix timestamp: ${value}`);
	return new Date(Math.trunc(seconds * 1000)).toISOString();
}

function maxUnix(...values) {
	const finite = values
		.filter((value) => value != null && Number.isFinite(Number(value)))
		.map(Number);
	return finite.length ? Math.max(...finite) : freezeSeconds;
}

function hashHex(value) {
	return crypto.createHash('sha256').update(value).digest('hex');
}

function stableValue(value) {
	if (Array.isArray(value)) return value.map(stableValue);
	if (value && typeof value === 'object') {
		return Object.fromEntries(
			Object.entries(value)
				.sort(([left], [right]) => compareText(left, right))
				.map(([key, entry]) => [key, stableValue(entry)])
		);
	}
	return value;
}

function canonicalJson(value) {
	return JSON.stringify(stableValue(value));
}

function uuidV7(namespace, legacyId, timestamp = freezeSeconds) {
	const milliseconds = BigInt(Math.trunc(Number(timestamp) * 1000));
	if (milliseconds < 0 || milliseconds > 0xffffffffffffn) {
		throw new Error(`UUID timestamp out of range for ${namespace}:${legacyId}`);
	}
	const digest = crypto
		.createHash('sha256')
		.update(`${manifest.snapshotChecksum}\0${guildId}\0${namespace}\0${legacyId}`)
		.digest();
	const bytes = Buffer.alloc(16);
	let remaining = milliseconds;
	for (let index = 5; index >= 0; index -= 1) {
		bytes[index] = Number(remaining & 0xffn);
		remaining >>= 8n;
	}
	digest.copy(bytes, 6, 0, 10);
	bytes[6] = (bytes[6] & 0x0f) | 0x70;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	const hex = bytes.toString('hex');
	return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function normalizeLane(value) {
	return value === 'ADC' ? 'BOTTOM' : LANES.includes(value) ? value : null;
}

function normalizeRiotId(value) {
	return String(value)
		.normalize('NFKC')
		.trim()
		.toLocaleLowerCase('und')
		.replaceAll('ß', 'ss')
		.replaceAll('ς', 'σ');
}

function sidePreset(team1Side) {
	if (team1Side === 'BLUE') return 'DEFAULT';
	if (team1Side === 'RED') return 'SWAPPED';
	throw new Error(`unsupported TEAM_1 side: ${team1Side}`);
}

function sideForTeam(team, preset) {
	if (!TEAMS.includes(team)) throw new Error(`unsupported team: ${team}`);
	return (team === 'TEAM_1') === (preset === 'DEFAULT') ? 'BLUE' : 'RED';
}

function parseBestOf(value, fallback = 3) {
	if (typeof value === 'string' && /^BO[135]$/.test(value)) return Number(value.slice(2));
	if ([1, 3, 5].includes(Number(value))) return Number(value);
	return fallback;
}

function redactSecrets(value) {
	if (Array.isArray(value)) return value.map(redactSecrets);
	if (!value || typeof value !== 'object') return value;
	return Object.fromEntries(
		Object.entries(value).map(([key, entry]) => [
			key,
			SECRET_KEY.test(key) ? '[REDACTED]' : redactSecrets(entry)
		])
	);
}

function ensure(condition, message) {
	if (!condition) throw new Error(message);
}

for (const [tableName, expected] of Object.entries(manifest.tables ?? {})) {
	const contents = fs.readFileSync(
		path.join(snapshotDir, 'raw-jsonl', `${tableName}.jsonl`),
		'utf8'
	);
	const count = contents.split('\n').filter(Boolean).length;
	ensure(count === expected.count, `source row count no longer matches manifest: ${tableName}`);
	ensure(
		hashHex(contents) === expected.checksum,
		`source checksum no longer matches manifest: ${tableName}`
	);
}
ensure(
	hashHex(
		canonicalJson({ schemaFingerprint: manifest.schemaFingerprint, tables: manifest.tables })
	) === manifest.snapshotChecksum,
	'source snapshot checksum no longer matches manifest'
);

const analysisChecksum = hashHex(fs.readFileSync(path.join(snapshotDir, 'analysis-report.json')));
const valkeyChecksum = hashHex(fs.readFileSync(path.join(snapshotDir, 'valkey-source.tsv')));
const championCatalogChecksum = hashHex(fs.readFileSync(path.resolve(args.champions)));

const source = Object.fromEntries(
	[
		'users',
		'riot_accounts',
		'seasons',
		'series',
		'series_participants',
		'games',
		'game_stats',
		'user_lane_mmr',
		'mmr_changes',
		'recruitments',
		'recruitment_participants',
		'recruitment_participant_roles',
		'admin_audit_log',
		'guild_kv',
		'game_picks',
		'game_bans',
		'user_champion_preferences',
		'auction_recruitments',
		'auction_recruitment_participants',
		'auction_tournaments',
		'auction_teams',
		'auction_team_members',
		'auction_bids',
		'auction_matches'
	].map((name) => [name, loadTable(name)])
);

const catalog = JSON.parse(fs.readFileSync(path.resolve(args.champions), 'utf8'));
const championAliases = new Map();
function championAliasKey(value) {
	return String(value)
		.normalize('NFKC')
		.trim()
		.toLocaleLowerCase('en-US')
		.replace(/[\s_'’.-]+/g, '');
}
for (const champion of Object.values(catalog.data ?? {})) {
	for (const alias of [champion.id, champion.key, champion.name]) {
		const key = championAliasKey(alias);
		const values = championAliases.get(key) ?? new Set();
		values.add(champion.id);
		championAliases.set(key, values);
	}
}
function champion(value) {
	if (value == null || value === '') return null;
	const values = championAliases.get(championAliasKey(value));
	if (values?.size !== 1) throw new Error(`champion is not uniquely resolvable: ${value}`);
	return [...values][0];
}

function loadValkeyRows() {
	return fs
		.readFileSync(path.join(snapshotDir, 'valkey-source.tsv'), 'utf8')
		.split('\n')
		.filter(Boolean)
		.map((line) => {
			const [encodedKey, encodedValue] = line.split('\t');
			return {
				key: Buffer.from(encodedKey, 'base64').toString('utf8'),
				value: Buffer.from(encodedValue, 'base64').toString('utf8'),
				updatedAt: freezeSeconds,
				source: 'VALKEY'
			};
		});
}

const authoritativeKv = new Map();
for (const row of source.guild_kv) {
	if (/^(entry|pickban|auction-lineup):/.test(row.k)) {
		authoritativeKv.set(row.k, {
			key: row.k,
			value: row.v,
			updatedAt: row.updated_at,
			source: 'D1'
		});
	}
}
for (const row of loadValkeyRows()) {
	ensure(!authoritativeKv.has(row.key), `duplicate authoritative draft key: ${row.key}`);
	authoritativeKv.set(row.key, row);
}

const tables = Object.fromEntries(TABLE_NAMES.map((name) => [name, []]));
const reportInfo = [];
const reportWarnings = [];
const runId = uuidV7('migration-run', `${manifest.snapshotChecksum}:${freezeIso}`, freezeSeconds);
const migrationActorId = `migration:${runId}`;

const usersById = indexBy(source.users, 'discord_id');
const accountsByUser = groupBy(source.riot_accounts, 'user_id');
const mmrSummaryByKey = new Map(
	source.user_lane_mmr.map((row) => [
		`${row.user_id}:${row.season_id}:${normalizeLane(row.role)}`,
		row
	])
);
const mainAccountByUser = new Map();

for (const legacyUser of source.users) {
	const userId = legacyUser.discord_id;
	const createdAt = isoFromUnix(legacyUser.created_at);
	const updatedSeconds = maxUnix(
		legacyUser.created_at,
		...(accountsByUser.get(userId) ?? []).map((account) => account.updated_at),
		legacyUser.deleted_at
	);
	const updatedAt = isoFromUnix(updatedSeconds);
	const deletedAt = legacyUser.deleted_at == null ? null : isoFromUnix(legacyUser.deleted_at);
	tables.users.push({
		user_id: userId,
		display_name: legacyUser.display_name,
		avatar_hash: null,
		created_at: createdAt,
		updated_at: updatedAt,
		deleted_at: deletedAt,
		identity_kind: 'DISCORD'
	});
	tables.user_discord_identities.push({
		user_id: userId,
		discord_id: userId,
		identity_kind: 'DISCORD',
		created_at: createdAt,
		updated_at: updatedAt
	});
	tables.user_privacy_settings.push({
		user_id: userId,
		public_profile_enabled: true,
		show_riot_ids: true,
		allow_external_record_search: false,
		updated_at: updatedAt
	});

	const accounts = [...(accountsByUser.get(userId) ?? [])].sort(
		(left, right) => left.created_at - right.created_at || compareText(left.puuid, right.puuid)
	);
	const explicitMain = accounts.filter((account) => account.is_main === 1);
	ensure(explicitMain.length <= 1, `multiple main Riot accounts for ${userId}`);
	const selectedMain = explicitMain[0] ?? accounts[0] ?? null;
	if (selectedMain) mainAccountByUser.set(userId, selectedMain);
	if (!explicitMain.length && selectedMain) {
		reportInfo.push({ reasonCode: 'INFERRED_MAIN_RIOT_ACCOUNT', entity: 'USER', legacyId: userId });
	}
	for (const account of accounts) {
		tables.riot_accounts.push({
			puuid: account.puuid,
			user_id: userId,
			platform: 'KR',
			routing_region: 'ASIA',
			game_name: account.game_name,
			tag_line: account.tag_line,
			normalized_game_name: normalizeRiotId(account.game_name),
			normalized_tag_line: normalizeRiotId(account.tag_line),
			profile_icon_id: account.profile_icon_id,
			is_main: account === selectedMain,
			main_position:
				normalizeLane(account.main_position) ?? (account.main_position === 'FLEX' ? 'FLEX' : null),
			main_position_updated_at:
				account.main_position_updated_at == null
					? null
					: isoFromUnix(account.main_position_updated_at),
			last_synced_at: null,
			created_at: isoFromUnix(account.created_at),
			updated_at: isoFromUnix(account.updated_at),
			origin: 'RIOT'
		});
	}

	const mainLane = normalizeLane(selectedMain?.main_position);
	tables.guild_members.push({
		guild_id: guildId,
		user_id: userId,
		display_name: legacyUser.display_name,
		status: deletedAt == null ? 'ACTIVE' : 'INACTIVE',
		main_lane: mainLane,
		first_seen_at: createdAt,
		registered_at: createdAt,
		discord_joined_at: null,
		last_active_at: updatedAt,
		last_synced_at: null,
		inactivated_at: deletedAt
	});
	if (deletedAt == null) {
		tables.user_guild_preferences.push({
			user_id: userId,
			default_guild_id: guildId,
			updated_at: updatedAt
		});
	}
}

const preferenceGroups = groupBy(
	source.user_champion_preferences,
	(row) => `${row.user_id}:${normalizeLane(row.role)}`
);
for (const [key, rows] of preferenceGroups) {
	const [userId, lane] = key.split(':');
	ensure(
		usersById.has(userId) && LANES.includes(lane),
		`invalid champion preference owner: ${key}`
	);
	const normalized = rows
		.map((row) => ({ championId: champion(row.champion_id), sourcePosition: row.position }))
		.sort(
			(left, right) =>
				left.sourcePosition - right.sourcePosition || compareText(left.championId, right.championId)
		);
	ensure(
		new Set(normalized.map((row) => row.championId)).size === normalized.length,
		`duplicate champion preference: ${key}`
	);
	ensure(normalized.length <= 10, `too many champion preferences: ${key}`);
	for (const [index, row] of normalized.entries()) {
		tables.user_champion_preferences.push({
			user_id: userId,
			lane,
			position: index + 1,
			champion_id: row.championId
		});
	}
}

const seasonsById = indexBy(source.seasons, 'id');
const seasonIds = new Map();
for (const legacySeason of source.seasons) {
	const id = uuidV7('season', legacySeason.id, legacySeason.created_at);
	seasonIds.set(legacySeason.id, id);
	tables.seasons.push({
		guild_id: guildId,
		id,
		name: legacySeason.name,
		status: legacySeason.ended_at == null ? 'ACTIVE' : 'CLOSED',
		started_at: isoFromUnix(legacySeason.started_at),
		ended_at: legacySeason.ended_at == null ? null : isoFromUnix(legacySeason.ended_at),
		created_by: null,
		created_at: isoFromUnix(legacySeason.created_at)
	});
}
ensure(
	tables.seasons.filter((season) => season.status === 'ACTIVE').length === 1,
	'exactly one active season is required'
);

function currentSnapshot(userId, legacySeasonId, preferredLane = null) {
	let lane =
		normalizeLane(preferredLane) ?? normalizeLane(mainAccountByUser.get(userId)?.main_position);
	let summary = lane == null ? null : mmrSummaryByKey.get(`${userId}:${legacySeasonId}:${lane}`);
	if (!lane || !summary) {
		const candidates = source.user_lane_mmr
			.filter((row) => row.user_id === userId && row.season_id === legacySeasonId)
			.sort(
				(left, right) =>
					right.updated_at - left.updated_at || LANES.indexOf(left.role) - LANES.indexOf(right.role)
			);
		if (!lane && candidates[0]) lane = normalizeLane(candidates[0].role);
		if (!summary && lane)
			summary = candidates.find((row) => normalizeLane(row.role) === lane) ?? null;
	}
	if (lane && summary) return { lane, mmr: summary.mmr, origin: 'INFERRED' };
	return { lane, mmr: null, origin: 'UNAVAILABLE' };
}

const allSeries = source.series.filter((row) => row.status === 'COMPLETED');
const allSeriesById = indexBy(allSeries, 'id');
const retainedSeriesIds = new Set(allSeries.map((row) => row.id));
const omittedIncompleteSeries = source.series.filter((row) => !retainedSeriesIds.has(row.id));
const omittedIncompleteSeriesIds = new Set(omittedIncompleteSeries.map((row) => row.id));
const seriesParticipantsById = groupBy(source.series_participants, 'series_id');
const omittedUnfinishedRecruitments = source.recruitments.filter(
	(row) => !retainedSeriesIds.has(row.id)
);
const omittedUnfinishedRecruitmentIds = new Set(omittedUnfinishedRecruitments.map((row) => row.id));
const omittedUnfinishedRecruitmentAssignments = new Map(
	omittedUnfinishedRecruitments.map((row) => {
		const draftRow = authoritativeKv.get(`entry:${row.id}`);
		const payload = draftRow == null ? null : JSON.parse(draftRow.value);
		const assignments = payload?.assignments ?? {};
		ensure(
			assignments && typeof assignments === 'object' && !Array.isArray(assignments),
			`invalid unfinished recruitment draft: ${row.id}`
		);
		return [row.id, Object.keys(assignments).length];
	})
);
const omittedUnfinishedRecruitmentSummary = {
	total: omittedUnfinishedRecruitments.length,
	linkedToIncompleteSeries: omittedUnfinishedRecruitments.filter((row) =>
		omittedIncompleteSeriesIds.has(row.id)
	).length,
	withoutCompletedSeries: omittedUnfinishedRecruitments.filter(
		(row) => !omittedIncompleteSeriesIds.has(row.id)
	).length,
	sourceStatusCounts: Object.fromEntries(
		['OPEN', 'CLOSED', 'CONVERTED', 'CANCELLED'].map((status) => [
			status,
			omittedUnfinishedRecruitments.filter((row) => row.status === status).length
		])
	),
	displayNumbers: omittedUnfinishedRecruitments
		.map((row) => row.id)
		.sort((left, right) => left - right),
	dependentRows: {
		participants: source.recruitment_participants.filter((row) =>
			omittedUnfinishedRecruitmentIds.has(row.recruitment_id)
		).length,
		preferredLanes: source.recruitment_participant_roles.filter((row) =>
			omittedUnfinishedRecruitmentIds.has(row.recruitment_id)
		).length,
		entryDrafts: omittedUnfinishedRecruitments.length,
		entryAssignments: [...omittedUnfinishedRecruitmentAssignments.values()].reduce(
			(sum, count) => sum + count,
			0
		)
	}
};
if (omittedUnfinishedRecruitments.length > 0) {
	reportInfo.push({
		reasonCode: 'OMITTED_NON_COMPLETED_RECRUITMENTS',
		entity: 'RECRUITMENT',
		legacyId: 'completed-history-only',
		rowCount: omittedUnfinishedRecruitments.length
	});
}
const omittedIncompleteSeriesGameIds = new Set(
	source.games
		.filter((row) => omittedIncompleteSeriesIds.has(row.ranked_series_id))
		.map((row) => row.id)
);
const omittedIncompleteSeriesSummary = {
	total: omittedIncompleteSeries.length,
	sourceStatusCounts: Object.fromEntries(
		['IN_PROGRESS', 'CANCELLED'].map((status) => [
			status,
			omittedIncompleteSeries.filter((row) => row.status === status).length
		])
	),
	displayNumbers: omittedIncompleteSeries.map((row) => row.id).sort((left, right) => left - right),
	dependentRows: {
		participants: source.series_participants.filter((row) =>
			omittedIncompleteSeriesIds.has(row.series_id)
		).length,
		games: omittedIncompleteSeriesGameIds.size,
		gameStats: source.game_stats.filter((row) => omittedIncompleteSeriesGameIds.has(row.game_id))
			.length,
		gamePicks: source.game_picks.filter((row) => omittedIncompleteSeriesGameIds.has(row.game_id))
			.length,
		gameBans: source.game_bans.filter((row) => omittedIncompleteSeriesGameIds.has(row.game_id))
			.length,
		mmrChanges: source.mmr_changes.filter((row) => omittedIncompleteSeriesGameIds.has(row.game_id))
			.length
	}
};
if (omittedIncompleteSeries.length > 0) {
	reportInfo.push({
		reasonCode: 'OMITTED_NON_COMPLETED_SERIES',
		entity: 'SERIES',
		legacyId: 'completed-history-only',
		rowCount: omittedIncompleteSeries.length
	});
}
const normalizedRecruitments = source.recruitments.filter((row) => retainedSeriesIds.has(row.id));
const normalizedRecruitmentParticipants = [...source.recruitment_participants];
const normalizedRecruitmentParticipantRoles = [...source.recruitment_participant_roles];
const recruitmentSourceIds = new Set(normalizedRecruitments.map((row) => row.id));
const recruitmentParticipantKeys = new Set(
	normalizedRecruitmentParticipants.map((row) => `${row.recruitment_id}:${row.user_id}`)
);
for (const legacySeries of allSeries) {
	const synthesizedRecruitment = !recruitmentSourceIds.has(legacySeries.id);
	if (synthesizedRecruitment) {
		normalizedRecruitments.push({
			id: legacySeries.id,
			season_id: legacySeries.season_id,
			target_count: 10,
			status: 'CONVERTED',
			converted_series_id: legacySeries.id,
			created_by: legacySeries.created_by,
			channel_id: null,
			message_id: null,
			created_at: legacySeries.started_at,
			updated_at: maxUnix(legacySeries.ended_at, legacySeries.deleted_at, legacySeries.started_at)
		});
		recruitmentSourceIds.add(legacySeries.id);
		reportInfo.push({
			reasonCode: 'SYNTHESIZED_HISTORICAL_RECRUITMENT',
			entity: 'RECRUITMENT',
			legacyId: String(legacySeries.id)
		});
	}
	for (const participant of seriesParticipantsById.get(legacySeries.id) ?? []) {
		const participantKey = `${legacySeries.id}:${participant.user_id}`;
		if (recruitmentParticipantKeys.has(participantKey)) continue;
		normalizedRecruitmentParticipants.push({
			recruitment_id: legacySeries.id,
			user_id: participant.user_id,
			joined_at: legacySeries.started_at
		});
		recruitmentParticipantKeys.add(participantKey);
		normalizedRecruitmentParticipantRoles.push({
			recruitment_id: legacySeries.id,
			user_id: participant.user_id,
			role: participant.role
		});
		if (!synthesizedRecruitment) {
			reportInfo.push({
				reasonCode: 'SYNTHESIZED_HISTORICAL_RECRUITMENT_PARTICIPANT',
				entity: 'RECRUITMENT_PARTICIPANT',
				legacyId: participantKey
			});
		}
	}
}
const recruitmentIds = new Map();
const recruitmentParticipantsById = groupBy(normalizedRecruitmentParticipants, 'recruitment_id');
const recruitmentRolesByParticipant = groupBy(
	normalizedRecruitmentParticipantRoles,
	(row) => `${row.recruitment_id}:${row.user_id}`
);
const gamesBySeries = groupBy(
	source.games.filter(
		(row) => row.ranked_series_id != null && retainedSeriesIds.has(row.ranked_series_id)
	),
	'ranked_series_id'
);
const retainedSeriesGames = source.games.filter(
	(row) => row.ranked_series_id != null && retainedSeriesIds.has(row.ranked_series_id)
);
const mmrChangesByGame = groupBy(source.mmr_changes, 'game_id');
const mmrAppliedGameIds = new Set();
for (const legacyGame of retainedSeriesGames) {
	const changes = mmrChangesByGame.get(legacyGame.id) ?? [];
	ensure(
		changes.length === 0 ||
			(changes.length === 10 && new Set(changes.map((row) => row.user_id)).size === 10),
		`series game has an incomplete MMR ledger: ${legacyGame.id}`
	);
	if (changes.length === 10) mmrAppliedGameIds.add(legacyGame.id);
}
const legacyMmrNotAppliedGameCount = retainedSeriesGames.filter(
	(row) => !mmrAppliedGameIds.has(row.id)
).length;
if (legacyMmrNotAppliedGameCount > 0) {
	reportInfo.push({
		reasonCode: 'HISTORICAL_GAME_MMR_NOT_APPLIED',
		entity: 'GAME',
		legacyId: 'all-history',
		rowCount: legacyMmrNotAppliedGameCount
	});
}

for (const legacy of normalizedRecruitments) {
	ensure(legacy.target_count === 10, `unsupported recruitment target count: ${legacy.id}`);
	const child = allSeriesById.get(legacy.id);
	const id = uuidV7('recruitment', legacy.id, legacy.created_at);
	recruitmentIds.set(legacy.id, id);
	let status = legacy.status;
	if (child) status = 'CONVERTED';
	else if (status === 'CONVERTED') status = 'CLOSED';
	const closedAt =
		status === 'CLOSED' || status === 'CONVERTED' ? isoFromUnix(legacy.updated_at) : null;
	const convertedAt = child ? isoFromUnix(child.started_at) : null;
	const cancelledAt = status === 'CANCELLED' ? isoFromUnix(legacy.updated_at) : null;
	tables.recruitments.push({
		guild_id: guildId,
		id,
		season_id: seasonIds.get(child?.season_id ?? legacy.season_id),
		display_number: legacy.id,
		title: `일반 내전 모집 #${legacy.id}`,
		target_count: 10,
		status,
		revision: 0,
		channel_id: legacy.channel_id,
		message_id: legacy.message_id,
		created_by: legacy.created_by,
		created_at: isoFromUnix(legacy.created_at),
		updated_by: null,
		updated_at: isoFromUnix(legacy.updated_at),
		closed_at: closedAt,
		converted_at: convertedAt,
		cancelled_at: cancelledAt,
		best_of: 3,
		hard_fearless: false
	});

	for (const participant of recruitmentParticipantsById.get(legacy.id) ?? []) {
		tables.recruitment_participants.push({
			guild_id: guildId,
			recruitment_id: id,
			user_id: participant.user_id,
			status: 'JOINED',
			joined_at: isoFromUnix(participant.joined_at),
			withdrawn_at: null
		});
		const roles = (recruitmentRolesByParticipant.get(`${legacy.id}:${participant.user_id}`) ?? [])
			.map((row) => normalizeLane(row.role))
			.filter(Boolean)
			.sort((left, right) => LANES.indexOf(left) - LANES.indexOf(right));
		ensure(
			new Set(roles).size === roles.length,
			`duplicate preferred lane: ${legacy.id}:${participant.user_id}`
		);
		for (const [index, lane] of roles.entries()) {
			tables.recruitment_preferred_lanes.push({
				guild_id: guildId,
				recruitment_id: id,
				user_id: participant.user_id,
				priority: index + 1,
				lane
			});
		}
	}

	const draftRow = authoritativeKv.get(`entry:${legacy.id}`);
	let assignments = {};
	let draftUpdatedAt = legacy.updated_at;
	if (child) {
		assignments = Object.fromEntries(
			(seriesParticipantsById.get(child.id) ?? []).map((row) => [
				row.user_id,
				`${row.team}_${normalizeLane(row.role)}`
			])
		);
		draftUpdatedAt = child.started_at;
	} else if (draftRow) {
		const payload = JSON.parse(draftRow.value);
		assignments = payload.assignments ?? {};
		draftUpdatedAt = draftRow.updatedAt;
	}
	tables.recruitment_entry_drafts.push({
		guild_id: guildId,
		recruitment_id: id,
		revision: 0,
		updated_by: null,
		updated_at: isoFromUnix(draftUpdatedAt)
	});
	const participantSet = new Set(
		(recruitmentParticipantsById.get(legacy.id) ?? []).map((row) => row.user_id)
	);
	const occupied = new Set();
	for (const [userId, slot] of Object.entries(assignments)) {
		const match = /^(TEAM_[12])_(TOP|JUNGLE|MID|BOTTOM|SUPPORT)$/.exec(slot);
		ensure(
			match && participantSet.has(userId) && !occupied.has(slot),
			`invalid entry draft assignment: ${legacy.id}:${userId}`
		);
		occupied.add(slot);
		tables.recruitment_entry_assignments.push({
			guild_id: guildId,
			recruitment_id: id,
			user_id: userId,
			team: match[1],
			lane: match[2]
		});
	}
}

const seriesIds = new Map();
let inferredBaselineSeriesMmrCount = 0;
for (const legacy of allSeries) {
	const id = uuidV7('series', legacy.id, legacy.started_at);
	seriesIds.set(legacy.id, id);
	const games = [...(gamesBySeries.get(legacy.id) ?? [])].sort(
		(left, right) => left.game_number - right.game_number
	);
	const score = { TEAM_1: 0, TEAM_2: 0 };
	for (const game of games) score[game.winning_team] += 1;
	const status =
		legacy.deleted_at != null && legacy.status === 'IN_PROGRESS' ? 'CANCELLED' : legacy.status;
	if (status !== legacy.status) {
		reportInfo.push({
			reasonCode: 'HISTORICAL_ACTIVE_STATE_CLOSED',
			entity: 'SERIES',
			legacyId: String(legacy.id),
			sourceStatus: legacy.status,
			targetStatus: status
		});
	}
	let terminalReason = null;
	let endedAt = null;
	if (status === 'COMPLETED') {
		endedAt = isoFromUnix(legacy.ended_at ?? legacy.deleted_at);
		if (legacy.winning_team == null) {
			ensure(score.TEAM_1 === score.TEAM_2, `completed series has an invalid draw: ${legacy.id}`);
			terminalReason = 'EARLY_DRAW';
		} else {
			terminalReason = score[legacy.winning_team] >= 2 ? 'SCORE_REACHED' : 'EARLY_WIN';
		}
	} else if (status === 'CANCELLED') {
		terminalReason = 'CANCELLED';
		endedAt = isoFromUnix(legacy.ended_at ?? legacy.deleted_at);
	}
	tables.series.push({
		guild_id: guildId,
		id,
		season_id: seasonIds.get(legacy.season_id),
		source_recruitment_id: recruitmentIds.get(legacy.id),
		display_number: legacy.id,
		title: `일반 내전 #${legacy.id}`,
		best_of: 3,
		status,
		winning_team: status === 'CANCELLED' ? null : legacy.winning_team,
		terminal_reason: terminalReason,
		revision: 0,
		started_at: isoFromUnix(legacy.started_at),
		ended_at: endedAt,
		created_by: legacy.created_by,
		end_card_channel_id: legacy.end_card_channel_id,
		end_card_message_id: legacy.end_card_message_id,
		updated_by: null,
		updated_at: isoFromUnix(maxUnix(legacy.ended_at, legacy.deleted_at, legacy.started_at)),
		deleted_at: null,
		hard_fearless: false
	});

	for (const participant of seriesParticipantsById.get(legacy.id) ?? []) {
		const firstChange = games
			.flatMap((game) => mmrChangesByGame.get(game.id) ?? [])
			.filter(
				(change) =>
					change.user_id === participant.user_id &&
					normalizeLane(change.role) === normalizeLane(participant.role)
			)
			.sort((left, right) => left.created_at - right.created_at || left.id - right.id)[0];
		const fallback = mmrSummaryByKey.get(
			`${participant.user_id}:${legacy.season_id}:${normalizeLane(participant.role)}`
		);
		const sourceMmr = firstChange?.mmr_before ?? fallback?.mmr;
		const mmr = sourceMmr ?? DEFAULT_MMR;
		if (sourceMmr == null) inferredBaselineSeriesMmrCount += 1;
		tables.series_participants.push({
			guild_id: guildId,
			series_id: id,
			user_id: participant.user_id,
			team: participant.team,
			lane: normalizeLane(participant.role),
			mmr_at_lock: mmr,
			mmr_snapshot_origin: firstChange ? 'CAPTURED' : 'INFERRED'
		});
	}
}
if (inferredBaselineSeriesMmrCount > 0) {
	reportInfo.push({
		reasonCode: 'INFERRED_BASELINE_SERIES_MMR',
		entity: 'SERIES_PARTICIPANT',
		legacyId: 'completed-history-only',
		rowCount: inferredBaselineSeriesMmrCount,
		mmr: DEFAULT_MMR
	});
}

const gameStatsByGame = groupBy(source.game_stats, 'game_id');
const gamePicksByGame = groupBy(source.game_picks, 'game_id');
const gameBansByGame = groupBy(source.game_bans, 'game_id');
const gameIds = new Map();
const gameInputIds = new Map();

function appendConfirmedGame(legacyGame, parent) {
	const preset = sidePreset(legacyGame.team1_side);
	const winningSide = sideForTeam(legacyGame.winning_team, preset);
	const gameId = uuidV7('game', legacyGame.id, legacyGame.played_at);
	const inputId = uuidV7('game-input-submitted', legacyGame.id, legacyGame.played_at);
	gameIds.set(legacyGame.id, gameId);
	gameInputIds.set(legacyGame.id, inputId);
	tables.game_inputs.push({
		guild_id: guildId,
		id: inputId,
		series_id: parent.kind === 'series' ? parent.id : null,
		auction_match_id: parent.kind === 'auction' ? parent.id : null,
		set_number: legacyGame.game_number,
		side_preset: preset,
		winning_side: winningSide,
		patch_version: null,
		status: 'SUBMITTED',
		updated_by: null,
		updated_at: isoFromUnix(legacyGame.played_at)
	});

	let stats = gameStatsByGame.get(legacyGame.id) ?? [];
	if (stats.length === 0 && parent.kind === 'series') {
		stats = (seriesParticipantsById.get(parent.legacySeriesId) ?? []).map((participant) => ({
			user_id: participant.user_id,
			team: participant.team,
			role: participant.role,
			champion_id: null,
			kills: null,
			deaths: null,
			assists: null,
			cs: null,
			won: participant.team === legacyGame.winning_team ? 1 : 0
		}));
		reportInfo.push({
			reasonCode: 'SYNTHESIZED_HISTORICAL_GAME_ROSTER',
			entity: 'GAME',
			legacyId: String(legacyGame.id)
		});
	}
	ensure(stats.length === 10, `confirmed game does not have ten participants: ${legacyGame.id}`);
	const statsBySlot = new Map(stats.map((row) => [`${row.team}:${normalizeLane(row.role)}`, row]));
	for (const stat of stats) {
		const championId = champion(stat.champion_id);
		tables.game_participants.push({
			guild_id: guildId,
			game_id: gameId,
			user_id: stat.user_id,
			team: stat.team,
			lane: normalizeLane(stat.role),
			champion_id: championId
		});
		tables.game_participant_stats.push({
			guild_id: guildId,
			game_id: gameId,
			user_id: stat.user_id,
			kills: stat.kills,
			deaths: stat.deaths,
			assists: stat.assists,
			cs: stat.cs
		});
	}
	for (const pick of gamePicksByGame.get(legacyGame.id) ?? []) {
		const stat = statsBySlot.get(`${pick.team}:${normalizeLane(pick.role)}`);
		ensure(stat, `pick cannot be resolved to a participant: ${legacyGame.id}`);
		const championId = champion(pick.champion_name);
		ensure(
			championId === champion(stat.champion_id),
			`pick/stat champion mismatch: ${legacyGame.id}:${stat.user_id}`
		);
		tables.game_input_picks.push({
			guild_id: guildId,
			game_input_id: inputId,
			user_id: stat.user_id,
			champion_id: championId
		});
	}
	for (const ban of gameBansByGame.get(legacyGame.id) ?? []) {
		const row = {
			guild_id: guildId,
			side: sideForTeam(ban.team, preset),
			slot: ban.position,
			champion_id: champion(ban.champion_name)
		};
		tables.game_input_bans.push({ ...row, game_input_id: inputId });
		tables.game_bans.push({ ...row, game_id: gameId });
	}
	tables.games.push({
		guild_id: guildId,
		id: gameId,
		season_id: parent.seasonId,
		series_id: parent.kind === 'series' ? parent.id : null,
		auction_match_id: parent.kind === 'auction' ? parent.id : null,
		source_input_id: inputId,
		game_number: legacyGame.game_number,
		side_preset: preset,
		winning_side: winningSide,
		duration_seconds: legacyGame.duration_sec > 0 ? legacyGame.duration_sec : null,
		patch_version: null,
		riot_match_id: legacyGame.riot_match_id,
		status: 'CONFIRMED',
		mmr_status:
			parent.kind === 'series' && mmrAppliedGameIds.has(legacyGame.id)
				? 'APPLIED'
				: 'LEGACY_NOT_APPLIED',
		played_at: isoFromUnix(legacyGame.played_at),
		confirmed_by: migrationActorId,
		confirmed_by_type: 'MIGRATION',
		confirmed_at: isoFromUnix(legacyGame.played_at),
		voided_by: null,
		voided_by_type: null,
		voided_at: null,
		void_reason: null
	});
}

for (const legacySeries of allSeries) {
	for (const game of gamesBySeries.get(legacySeries.id) ?? []) {
		appendConfirmedGame(game, {
			kind: 'series',
			id: seriesIds.get(legacySeries.id),
			seasonId: seasonIds.get(legacySeries.season_id),
			legacySeriesId: legacySeries.id
		});
	}
}

let omittedSeriesDraftInputCount = 0;
for (const legacySeries of source.series) {
	const draftRow = authoritativeKv.get(`pickban:${legacySeries.id}`);
	if (!draftRow) continue;
	const payload = JSON.parse(draftRow.value);
	const confirmedNumbers = new Set(
		source.games
			.filter((game) => game.ranked_series_id === legacySeries.id)
			.map((game) => game.game_number)
	);
	for (const draft of payload.games ?? []) {
		if (!confirmedNumbers.has(draft.gameNumber)) omittedSeriesDraftInputCount += 1;
	}
}
if (omittedSeriesDraftInputCount > 0) {
	reportInfo.push({
		reasonCode: 'OMITTED_UNSUBMITTED_GAME_INPUTS',
		entity: 'GAME_INPUT',
		legacyId: 'completed-history-only',
		rowCount: omittedSeriesDraftInputCount
	});
}

const legacyGamesById = indexBy(source.games, 'id');
const mmrEventsByLane = groupBy(
	source.mmr_changes.filter(
		(row) => gameIds.has(row.game_id) && mmrAppliedGameIds.has(row.game_id)
	),
	(row) => `${row.user_id}:${row.season_id}:${normalizeLane(row.role)}`
);
const mmrKeys = new Set([...mmrEventsByLane.keys(), ...mmrSummaryByKey.keys()]);
for (const key of [...mmrKeys].sort(compareText)) {
	const [userId, legacySeasonIdText, lane] = key.split(':');
	const legacySeasonId = Number(legacySeasonIdText);
	const events = [...(mmrEventsByLane.get(key) ?? [])].sort(
		(left, right) => left.created_at - right.created_at || left.id - right.id
	);
	const summary = mmrSummaryByKey.get(key);
	let currentMmr = 1500;
	let gamesPlayed = 0;
	let wins = 0;
	let revision = 0;

	function appendSeed(targetMmr, gamesDelta, winsDelta, reason, timestamp, sequence) {
		revision += 1;
		const id = uuidV7('mmr-seed', `${key}:${sequence}`, timestamp);
		tables.mmr_ledger_entries.push({
			guild_id: guildId,
			id,
			season_id: seasonIds.get(legacySeasonId),
			user_id: userId,
			lane,
			lane_revision: revision,
			source_type: 'MIGRATION_SEED',
			source_id: id,
			game_id: null,
			opponent_user_id: null,
			mmr_before: currentMmr,
			mmr_after: targetMmr,
			delta: targetMmr - currentMmr,
			games_played_delta: gamesDelta,
			wins_delta: winsDelta,
			reverses_entry_id: null,
			reversed_source_type: null,
			actor_type: 'MIGRATION',
			actor_id: migrationActorId,
			note: reason,
			request_id: null,
			created_at: isoFromUnix(timestamp)
		});
		currentMmr = targetMmr;
		gamesPlayed += gamesDelta;
		wins += winsDelta;
	}

	for (const [eventIndex, event] of events.entries()) {
		if (Math.abs(currentMmr - event.mmr_before) > 0.000001) {
			appendSeed(
				event.mmr_before,
				0,
				0,
				'MOOKBOT_v3 ledger gap reconciliation',
				event.created_at,
				`gap:${eventIndex}`
			);
			reportInfo.push({
				reasonCode: 'MIGRATION_SEED_GAP',
				entity: 'MMR',
				legacyId: `${key}:${event.id}`
			});
		}
		const legacyGame = legacyGamesById.get(event.game_id);
		const stat = (gameStatsByGame.get(event.game_id) ?? []).find((row) => row.user_id === userId);
		ensure(legacyGame && stat, `MMR event is missing its game participant: ${event.id}`);
		revision += 1;
		tables.mmr_ledger_entries.push({
			guild_id: guildId,
			id: uuidV7('mmr-change', event.id, event.created_at),
			season_id: seasonIds.get(legacySeasonId),
			user_id: userId,
			lane,
			lane_revision: revision,
			source_type: 'GAME_RESULT',
			source_id: gameIds.get(event.game_id),
			game_id: gameIds.get(event.game_id),
			opponent_user_id: event.opponent_id,
			mmr_before: event.mmr_before,
			mmr_after: event.mmr_after,
			delta: event.delta,
			games_played_delta: 1,
			wins_delta: stat.won === 1 ? 1 : 0,
			reverses_entry_id: null,
			reversed_source_type: null,
			actor_type: 'MIGRATION',
			actor_id: migrationActorId,
			note: null,
			request_id: null,
			created_at: isoFromUnix(event.created_at)
		});
		currentMmr = event.mmr_after;
		gamesPlayed += 1;
		wins += stat.won === 1 ? 1 : 0;
	}
	if (
		summary &&
		(Math.abs(currentMmr - summary.mmr) > 0.000001 ||
			gamesPlayed !== summary.games_played ||
			wins !== summary.wins)
	) {
		appendSeed(
			summary.mmr,
			summary.games_played - gamesPlayed,
			summary.wins - wins,
			'MOOKBOT_v3 current summary reconciliation',
			summary.updated_at,
			'tail'
		);
		reportInfo.push({ reasonCode: 'MIGRATION_SEED_TAIL', entity: 'MMR', legacyId: key });
	}
	ensure(revision > 0, `MMR summary has no ledger representation: ${key}`);
	tables.user_lane_mmr.push({
		guild_id: guildId,
		season_id: seasonIds.get(legacySeasonId),
		user_id: userId,
		lane,
		mmr: summary?.mmr ?? currentMmr,
		games_played: summary?.games_played ?? gamesPlayed,
		wins: summary?.wins ?? wins,
		revision,
		updated_at: isoFromUnix(summary?.updated_at ?? events.at(-1)?.created_at)
	});
}

const sourceAuctionMatchesByTournament = groupBy(source.auction_matches, 'tournament_id');
const allTournaments = source.auction_tournaments.filter((row) => {
	const matches = sourceAuctionMatchesByTournament.get(row.id) ?? [];
	return (
		row.status === 'COMPLETED' &&
		matches.length > 0 &&
		matches.every((match) => match.status === 'COMPLETED')
	);
});
const allTournamentById = indexBy(allTournaments, 'id');
const retainedAuctionTournamentIds = new Set(allTournaments.map((row) => row.id));
const omittedIncompleteAuctionTournaments = source.auction_tournaments.filter(
	(row) => !retainedAuctionTournamentIds.has(row.id)
);
const omittedIncompleteAuctionTournamentIds = new Set(
	omittedIncompleteAuctionTournaments.map((row) => row.id)
);
const retainedAuctionMatches = source.auction_matches.filter((row) =>
	retainedAuctionTournamentIds.has(row.tournament_id)
);
const retainedAuctionMatchIds = new Set(retainedAuctionMatches.map((row) => row.id));
const omittedAuctionMatchIds = new Set(
	source.auction_matches.filter((row) => !retainedAuctionMatchIds.has(row.id)).map((row) => row.id)
);
const legacyAuctionTeamsByTournament = groupBy(source.auction_teams, 'tournament_id');
const legacyAuctionMembersByTeam = groupBy(source.auction_team_members, 'team_id');
const legacyAuctionBidsByTournament = groupBy(source.auction_bids, 'tournament_id');
const legacyAuctionMatchesByTournament = groupBy(retainedAuctionMatches, 'tournament_id');
const normalizedAuctionRecruitments = source.auction_recruitments.filter((row) =>
	retainedAuctionTournamentIds.has(row.id)
);
const normalizedAuctionRecruitmentParticipants = [...source.auction_recruitment_participants];
const omittedAuctionRecruitmentIds = new Set(
	source.auction_recruitments
		.filter((row) => !retainedAuctionTournamentIds.has(row.id))
		.map((row) => row.id)
);
const omittedIncompleteAuctionWorkflowSummary = {
	tournaments: omittedIncompleteAuctionTournaments.length,
	recruitments: omittedAuctionRecruitmentIds.size,
	sourceTournamentStatusCounts: Object.fromEntries(
		['RECRUITING', 'BIDDING', 'IN_GAME', 'CANCELLED'].map((status) => [
			status,
			omittedIncompleteAuctionTournaments.filter((row) => row.status === status).length
		])
	),
	displayNumbers: omittedIncompleteAuctionTournaments
		.map((row) => row.id)
		.sort((left, right) => left - right),
	dependentRows: {
		recruitmentParticipants: source.auction_recruitment_participants.filter((row) =>
			omittedAuctionRecruitmentIds.has(row.recruitment_id)
		).length,
		teams: source.auction_teams.filter((row) =>
			omittedIncompleteAuctionTournamentIds.has(row.tournament_id)
		).length,
		bids: source.auction_bids.filter((row) =>
			omittedIncompleteAuctionTournamentIds.has(row.tournament_id)
		).length,
		matches: omittedAuctionMatchIds.size,
		games: source.games.filter((row) => omittedAuctionMatchIds.has(row.auction_match_id)).length
	}
};
if (omittedIncompleteAuctionTournaments.length > 0 || omittedAuctionRecruitmentIds.size > 0) {
	reportInfo.push({
		reasonCode: 'OMITTED_NON_COMPLETED_AUCTION_WORKFLOWS',
		entity: 'AUCTION_TOURNAMENT',
		legacyId: 'completed-history-only',
		rowCount: omittedIncompleteAuctionTournaments.length
	});
}
const auctionRecruitmentSourceIds = new Set(normalizedAuctionRecruitments.map((row) => row.id));
for (const legacyTournament of allTournaments) {
	if (auctionRecruitmentSourceIds.has(legacyTournament.id)) continue;
	const teams = legacyAuctionTeamsByTournament.get(legacyTournament.id) ?? [];
	const participantUserIds = [
		...new Set(
			teams.flatMap((team) =>
				(legacyAuctionMembersByTeam.get(team.id) ?? []).map((member) => member.user_id)
			)
		)
	];
	ensure(
		participantUserIds.length === Number(legacyTournament.format),
		`historical auction recruitment cannot be reconstructed: ${legacyTournament.id}`
	);
	normalizedAuctionRecruitments.push({
		id: legacyTournament.id,
		season_id: legacyTournament.season_id,
		target_count: Number(legacyTournament.format),
		status: 'CONVERTED',
		converted_tournament_id: legacyTournament.id,
		created_by: legacyTournament.created_by,
		channel_id: null,
		message_id: null,
		created_at: legacyTournament.started_at,
		updated_at: maxUnix(
			legacyTournament.ended_at,
			legacyTournament.deleted_at,
			legacyTournament.started_at
		)
	});
	for (const userId of participantUserIds) {
		normalizedAuctionRecruitmentParticipants.push({
			recruitment_id: legacyTournament.id,
			user_id: userId,
			joined_at: legacyTournament.started_at
		});
	}
	reportInfo.push({
		reasonCode: 'SYNTHESIZED_HISTORICAL_AUCTION_RECRUITMENT',
		entity: 'AUCTION_RECRUITMENT',
		legacyId: String(legacyTournament.id)
	});
}
const auctionParticipantsById = groupBy(normalizedAuctionRecruitmentParticipants, 'recruitment_id');
const auctionRecruitmentIds = new Map();
const auctionTournamentIds = new Map();
const auctionTeamIds = new Map();
const auctionMatchIds = new Map();
const auctionMemberIds = new Map();

function tournamentFormat(value) {
	if (value === 10 || value === 'TEN_PERSON') return 'TEN_PERSON';
	if (value === 20 || value === 'TWENTY_PERSON') return 'TWENTY_PERSON';
	throw new Error(`unsupported tournament format: ${value}`);
}

function lineupPayload(legacyTournamentId) {
	const row = authoritativeKv.get(`auction-lineup:${legacyTournamentId}`);
	if (!row) return null;
	return { row, assignments: JSON.parse(row.value).assignments ?? {} };
}

function lineupLaneForUser(legacyTournamentId, userId) {
	const payload = lineupPayload(legacyTournamentId);
	if (!payload) return null;
	for (const lanes of Object.values(payload.assignments)) {
		for (const [lane, assignedUserId] of Object.entries(lanes)) {
			if (assignedUserId === userId) return normalizeLane(lane);
		}
	}
	return null;
}

for (const legacy of normalizedAuctionRecruitments) {
	const child = allTournamentById.get(legacy.id);
	const format = tournamentFormat(child?.format ?? legacy.target_count);
	const id = uuidV7('auction-recruitment', legacy.id, legacy.created_at);
	auctionRecruitmentIds.set(legacy.id, id);
	let status = legacy.status;
	if (child) status = 'CONVERTED';
	else if (status === 'CONVERTED') status = 'CLOSED';
	tables.auction_recruitments.push({
		guild_id: guildId,
		id,
		season_id: seasonIds.get(child?.season_id ?? legacy.season_id),
		display_number: legacy.id,
		title: `경매 내전 모집 #${legacy.id}`,
		format,
		target_count: format === 'TEN_PERSON' ? 10 : 20,
		status,
		revision: 0,
		channel_id: legacy.channel_id,
		message_id: legacy.message_id,
		created_by: legacy.created_by,
		created_at: isoFromUnix(legacy.created_at),
		updated_by: null,
		updated_at: isoFromUnix(legacy.updated_at),
		closed_at: ['CLOSED', 'CONVERTED'].includes(status) ? isoFromUnix(legacy.updated_at) : null,
		converted_at: child ? isoFromUnix(child.started_at) : null,
		cancelled_at: status === 'CANCELLED' ? isoFromUnix(legacy.updated_at) : null,
		semifinal_best_of: format === 'TWENTY_PERSON' ? 3 : null,
		final_best_of: 3,
		hard_fearless: false
	});

	const participants = auctionParticipantsById.get(legacy.id) ?? [];
	for (const participant of participants) {
		const snapshot = currentSnapshot(
			participant.user_id,
			child?.season_id ?? legacy.season_id,
			child ? lineupLaneForUser(child.id, participant.user_id) : null
		);
		tables.auction_recruitment_participants.push({
			guild_id: guildId,
			recruitment_id: id,
			user_id: participant.user_id,
			status: 'JOINED',
			declared_lane: snapshot.lane,
			mmr_at_join: snapshot.mmr,
			snapshot_origin: snapshot.origin,
			joined_at: isoFromUnix(participant.joined_at),
			withdrawn_at: null
		});
	}

	tables.auction_setup_drafts.push({
		guild_id: guildId,
		recruitment_id: id,
		revision: 0,
		updated_by: null,
		updated_at: isoFromUnix(legacy.updated_at)
	});
	const sourceTeams = child ? (legacyAuctionTeamsByTournament.get(child.id) ?? []) : [];
	const expectedTeams = format === 'TEN_PERSON' ? 2 : 4;
	for (let teamIndex = 1; teamIndex <= expectedTeams; teamIndex += 1) {
		const sourceTeam = sourceTeams.find((team) => team.team_index === teamIndex);
		tables.auction_setup_teams.push({
			guild_id: guildId,
			recruitment_id: id,
			team_index: teamIndex,
			team_name: sourceTeam?.team_name?.trim() || `팀 ${teamIndex}`,
			captain_user_id: sourceTeam?.captain_user_id ?? null,
			initial_points: sourceTeam?.initial_points ?? 1000
		});
	}
}

for (const legacy of allTournaments) {
	const id = uuidV7('auction-tournament', legacy.id, legacy.started_at);
	auctionTournamentIds.set(legacy.id, id);
	const format = tournamentFormat(legacy.format);
	const matches = legacyAuctionMatchesByTournament.get(legacy.id) ?? [];
	const firstMatch = [...matches].sort(
		(left, right) => (left.started_at ?? Infinity) - (right.started_at ?? Infinity)
	)[0];
	const bracketLockedAt = firstMatch
		? isoFromUnix(firstMatch.started_at ?? legacy.started_at)
		: null;
	const bracketLockedBy = firstMatch?.created_by ?? null;
	const status = legacy.status;
	tables.auction_tournaments.push({
		guild_id: guildId,
		id,
		season_id: seasonIds.get(legacy.season_id),
		source_recruitment_id: auctionRecruitmentIds.get(legacy.id),
		display_number: legacy.id,
		title: `경매 내전 #${legacy.id}`,
		format,
		status,
		revision: 0,
		bracket_locked_at: bracketLockedAt,
		bracket_locked_by: bracketLockedBy,
		champion_team_id: null,
		started_at: isoFromUnix(legacy.started_at),
		ended_at: ['COMPLETED', 'CANCELLED'].includes(status)
			? isoFromUnix(legacy.ended_at ?? legacy.deleted_at)
			: null,
		created_by: legacy.created_by,
		updated_by: null,
		updated_at: isoFromUnix(
			maxUnix(legacy.ended_at, legacy.deleted_at, firstMatch?.started_at, legacy.started_at)
		),
		end_card_channel_id: legacy.end_card_channel_id,
		end_card_message_id: legacy.end_card_message_id,
		deleted_at: null,
		semifinal_best_of: format === 'TWENTY_PERSON' ? 3 : null,
		final_best_of: 3,
		hard_fearless: false
	});

	const teams = [...(legacyAuctionTeamsByTournament.get(legacy.id) ?? [])].sort(
		(left, right) => left.team_index - right.team_index
	);
	const teamByLegacyId = indexBy(teams, 'id');
	const captains = new Set(teams.map((team) => team.captain_user_id));
	const participantRows = auctionParticipantsById.get(legacy.id) ?? [];
	const participants = new Set(participantRows.map((row) => row.user_id));
	for (const team of teams) {
		const teamId = uuidV7('auction-team', team.id, legacy.started_at);
		auctionTeamIds.set(team.id, teamId);
		const finalBids = (legacyAuctionBidsByTournament.get(legacy.id) ?? []).filter(
			(bid) => bid.is_final === 1 && bid.team_id === team.id
		);
		tables.auction_teams.push({
			guild_id: guildId,
			tournament_id: id,
			id: teamId,
			team_index: team.team_index,
			team_name: team.team_name?.trim() || `팀 ${team.team_index}`,
			initial_points: team.initial_points,
			current_points: team.current_points,
			point_revision: finalBids.length,
			created_at: isoFromUnix(legacy.started_at)
		});
	}

	const finalBids = [...(legacyAuctionBidsByTournament.get(legacy.id) ?? [])]
		.filter((bid) => bid.is_final === 1)
		.sort((left, right) => left.created_at - right.created_at || left.id - right.id);
	const finalBidByUser = groupBy(finalBids, 'target_user_id');
	const assignedUnits = [];
	for (const team of teams) {
		const members = legacyAuctionMembersByTeam.get(team.id) ?? [];
		for (const member of members) {
			if (member.user_id === team.captain_user_id) continue;
			const bids = finalBidByUser.get(member.user_id) ?? [];
			if (member.acquired_via === 'BID') {
				ensure(
					bids.length === 1 && bids[0].team_id === team.id,
					`bid/member mismatch: ${legacy.id}:${member.user_id}`
				);
			}
			assignedUnits.push({
				targetUserId: member.user_id,
				team,
				member,
				bid: member.acquired_via === 'BID' ? bids[0] : null,
				kind: member.acquired_via === 'BID' ? 'AWARD_BID' : 'ASSIGN_CURRENT',
				timestamp: member.acquired_via === 'BID' ? bids[0].created_at : legacy.started_at,
				legacySort: member.acquired_via === 'BID' ? bids[0].id : member.user_id
			});
		}
	}
	ensure(
		new Set(assignedUnits.map((unit) => unit.targetUserId)).size === assignedUnits.length,
		`duplicate auction assignee: ${legacy.id}`
	);
	const currentTarget = legacy.deleted_at == null ? legacy.current_bid_target_user_id : null;
	if (currentTarget != null) {
		ensure(
			participants.has(currentTarget) && !captains.has(currentTarget),
			`invalid current auction target: ${legacy.id}`
		);
		ensure(
			!assignedUnits.some((unit) => unit.targetUserId === currentTarget),
			`current target is already assigned: ${legacy.id}`
		);
	}

	let sourceQueue = [];
	if (legacy.bid_candidate_queue_user_ids != null)
		sourceQueue = JSON.parse(legacy.bid_candidate_queue_user_ids);
	const manualUsers = assignedUnits
		.filter((unit) => unit.kind === 'ASSIGN_CURRENT')
		.map((unit) => unit.targetUserId)
		.sort(compareText);
	const eventUsers = [
		...sourceQueue,
		...(currentTarget ? [currentTarget] : []),
		...finalBids.map((bid) => bid.target_user_id),
		...manualUsers,
		...participantRows
			.filter((participant) => !captains.has(participant.user_id))
			.sort(
				(left, right) =>
					left.joined_at - right.joined_at || compareText(left.user_id, right.user_id)
			)
			.map((participant) => participant.user_id)
	];
	const orderedCandidateUsers = [...new Set(eventUsers)].filter((userId) => !captains.has(userId));
	ensure(
		orderedCandidateUsers.length === participantRows.length - captains.size,
		`auction candidate population mismatch: ${legacy.id}`
	);

	const units = [...assignedUnits];
	if (currentTarget) {
		units.push({
			targetUserId: currentTarget,
			team: null,
			member: null,
			bid: null,
			kind: null,
			timestamp: freezeSeconds,
			legacySort: currentTarget
		});
	}
	units.sort(
		(left, right) =>
			left.timestamp - right.timestamp ||
			compareText(left.legacySort, right.legacySort) ||
			compareText(left.targetUserId, right.targetUserId)
	);

	const actionByUser = new Map();
	const actionRows = [];
	for (const [drawIndex, unit] of units.entries()) {
		const lotId = uuidV7('auction-lot', `${legacy.id}:${unit.targetUserId}`, unit.timestamp);
		const closed = unit.kind != null;
		tables.auction_lots.push({
			guild_id: guildId,
			tournament_id: id,
			id: lotId,
			draw_sequence: drawIndex + 1,
			target_user_id: unit.targetUserId,
			status: !closed ? 'ACTIVE' : unit.kind === 'AWARD_BID' ? 'AWARDED' : 'ASSIGNED',
			opened_by_type: 'MIGRATION',
			opened_by_id: migrationActorId,
			opened_at: isoFromUnix(unit.timestamp),
			closed_by_type: closed ? 'MIGRATION' : null,
			closed_by_id: closed ? migrationActorId : null,
			closed_at: closed ? isoFromUnix(unit.timestamp) : null
		});
		actionRows.push({
			key: `${unit.targetUserId}:draw`,
			kindOrder: 1,
			timestamp: unit.timestamp,
			legacySort: unit.legacySort,
			row: {
				guild_id: guildId,
				tournament_id: id,
				id: uuidV7('auction-action-draw', `${legacy.id}:${unit.targetUserId}`, unit.timestamp),
				sequence: 0,
				kind: 'DRAW_NEXT',
				target_user_id: unit.targetUserId,
				team_id: null,
				lot_id: lotId,
				points: null,
				reverses_action_id: null,
				reversed_action_kind: null,
				actor_type: 'MIGRATION',
				actor_id: migrationActorId,
				created_at: isoFromUnix(unit.timestamp)
			}
		});
		if (unit.kind) {
			const actionId = uuidV7(
				'auction-action-acquire',
				`${legacy.id}:${unit.targetUserId}`,
				unit.timestamp
			);
			actionByUser.set(unit.targetUserId, { id: actionId, kind: unit.kind });
			actionRows.push({
				key: `${unit.targetUserId}:acquire`,
				kindOrder: unit.kind === 'AWARD_BID' ? 2 : 3,
				timestamp: unit.timestamp,
				legacySort: unit.legacySort,
				row: {
					guild_id: guildId,
					tournament_id: id,
					id: actionId,
					sequence: 0,
					kind: unit.kind,
					target_user_id: unit.targetUserId,
					team_id: auctionTeamIds.get(unit.team.id),
					lot_id: lotId,
					points: unit.kind === 'AWARD_BID' ? unit.bid.points : null,
					reverses_action_id: null,
					reversed_action_kind: null,
					actor_type: 'MIGRATION',
					actor_id: migrationActorId,
					created_at: isoFromUnix(unit.timestamp)
				}
			});
		}
	}
	actionRows.sort(
		(left, right) =>
			left.timestamp - right.timestamp ||
			left.kindOrder - right.kindOrder ||
			compareText(left.legacySort, right.legacySort) ||
			compareText(left.key, right.key)
	);
	for (const [index, action] of actionRows.entries()) {
		action.row.sequence = index + 1;
		tables.auction_actions.push(action.row);
	}

	for (const [queuePosition, userId] of orderedCandidateUsers.entries()) {
		const assigned = assignedUnits.some((unit) => unit.targetUserId === userId);
		const snapshot = currentSnapshot(
			userId,
			legacy.season_id,
			lineupLaneForUser(legacy.id, userId)
		);
		tables.auction_bid_candidates.push({
			guild_id: guildId,
			tournament_id: id,
			user_id: userId,
			declared_lane: snapshot.lane,
			mmr_at_start: snapshot.mmr,
			snapshot_origin: snapshot.origin,
			queue_position: queuePosition,
			state: assigned ? 'ASSIGNED' : userId === currentTarget ? 'ACTIVE' : 'QUEUED',
			draw_count: assigned || userId === currentTarget ? 1 : 0,
			updated_at: isoFromUnix(freezeSeconds)
		});
	}

	for (const team of teams) {
		const teamId = auctionTeamIds.get(team.id);
		const members = legacyAuctionMembersByTeam.get(team.id) ?? [];
		const captain = members.find((member) => member.user_id === team.captain_user_id);
		ensure(captain, `auction captain member is missing: ${team.id}`);
		const nonCaptains = members
			.filter((member) => member.user_id !== team.captain_user_id)
			.sort((left, right) => {
				const leftUnit = assignedUnits.find((unit) => unit.targetUserId === left.user_id);
				const rightUnit = assignedUnits.find((unit) => unit.targetUserId === right.user_id);
				return leftUnit.timestamp - rightUnit.timestamp || compareText(left.user_id, right.user_id);
			});
		for (const [index, member] of [captain, ...nonCaptains].entries()) {
			const isCaptain = index === 0;
			const action = actionByUser.get(member.user_id);
			const snapshot = currentSnapshot(
				member.user_id,
				legacy.season_id,
				lineupLaneForUser(legacy.id, member.user_id)
			);
			const memberId = uuidV7(
				'auction-team-member',
				`${team.id}:${member.user_id}`,
				legacy.started_at
			);
			auctionMemberIds.set(`${team.id}:${member.user_id}`, memberId);
			tables.auction_team_members.push({
				guild_id: guildId,
				tournament_id: id,
				team_id: teamId,
				id: memberId,
				user_id: member.user_id,
				roster_slot: index + 1,
				acquired_via: isCaptain ? 'CAPTAIN' : member.acquired_via,
				declared_lane: snapshot.lane,
				mmr_at_acquisition: snapshot.mmr,
				snapshot_origin: snapshot.origin,
				acquisition_action_id: isCaptain ? null : action.id,
				acquisition_action_kind: isCaptain ? null : action.kind,
				joined_at: isoFromUnix(
					isCaptain
						? legacy.started_at
						: assignedUnits.find((unit) => unit.targetUserId === member.user_id).timestamp
				),
				released_at: null
			});
		}

		let points = team.initial_points;
		tables.auction_point_ledger.push({
			guild_id: guildId,
			tournament_id: id,
			id: uuidV7('auction-point-initial', team.id, legacy.started_at),
			team_id: teamId,
			point_revision: 0,
			reason: 'INITIAL',
			source_action_id: null,
			source_action_kind: null,
			points_before: 0,
			points_after: team.initial_points,
			delta: team.initial_points,
			reverses_entry_id: null,
			reversed_reason: null,
			actor_type: 'MIGRATION',
			actor_id: migrationActorId,
			created_at: isoFromUnix(legacy.started_at)
		});
		const teamBids = finalBids.filter((bid) => bid.team_id === team.id);
		for (const [index, bid] of teamBids.entries()) {
			const action = actionByUser.get(bid.target_user_id);
			const after = points - bid.points;
			ensure(after >= 0, `auction points became negative: ${team.id}`);
			tables.auction_point_ledger.push({
				guild_id: guildId,
				tournament_id: id,
				id: uuidV7('auction-point-bid', bid.id, bid.created_at),
				team_id: teamId,
				point_revision: index + 1,
				reason: 'BID_DEBIT',
				source_action_id: action.id,
				source_action_kind: 'AWARD_BID',
				points_before: points,
				points_after: after,
				delta: -bid.points,
				reverses_entry_id: null,
				reversed_reason: null,
				actor_type: 'MIGRATION',
				actor_id: migrationActorId,
				created_at: isoFromUnix(bid.created_at)
			});
			points = after;
		}
		ensure(points === team.current_points, `auction point reconciliation failed: ${team.id}`);
	}

	for (const legacyMatch of matches) {
		const matchId = uuidV7(
			'auction-match',
			legacyMatch.id,
			legacyMatch.started_at ?? legacy.started_at
		);
		auctionMatchIds.set(legacyMatch.id, matchId);
		const payload = lineupPayload(legacy.id);
		const hasLineup = Boolean(
			payload &&
			[legacyMatch.team1_id, legacyMatch.team2_id].every((teamId) =>
				LANES.every((lane) => payload.assignments[String(teamId)]?.[lane])
			)
		);
		const auctionGames = source.games.filter((game) => game.auction_match_id === legacyMatch.id);
		let status = legacyMatch.status;
		if (legacyMatch.deleted_at != null && !['COMPLETED', 'CANCELLED'].includes(status)) {
			status = 'CANCELLED';
			reportInfo.push({
				reasonCode: 'HISTORICAL_ACTIVE_STATE_CLOSED',
				entity: 'AUCTION_MATCH',
				legacyId: String(legacyMatch.id),
				sourceStatus: legacyMatch.status,
				targetStatus: status
			});
		}
		if (status === 'IN_PROGRESS' && auctionGames.length === 0)
			status = hasLineup ? 'READY' : 'LINEUP_PENDING';
		const lockRequired = ['READY', 'IN_PROGRESS', 'COMPLETED'].includes(status);
		ensure(
			!lockRequired || hasLineup || auctionGames.length > 0,
			`locked auction match has no reconstructable lineup: ${legacyMatch.id}`
		);
		const lockAt = lockRequired
			? isoFromUnix(payload?.row.updatedAt ?? legacyMatch.started_at ?? legacy.started_at)
			: null;
		const lockBy = lockRequired ? legacyMatch.created_by : null;
		const winningTeamId =
			legacyMatch.winning_team === 'TEAM_1'
				? auctionTeamIds.get(legacyMatch.team1_id)
				: legacyMatch.winning_team === 'TEAM_2'
					? auctionTeamIds.get(legacyMatch.team2_id)
					: null;
		tables.auction_matches.push({
			guild_id: guildId,
			tournament_id: id,
			id: matchId,
			season_id: seasonIds.get(legacy.season_id),
			match_key:
				legacyMatch.round === 'SINGLE'
					? 'FINAL'
					: legacyMatch.bracket_index === 1
						? 'SEMIFINAL_A'
						: 'SEMIFINAL_B',
			best_of: parseBestOf(legacyMatch.format),
			left_team_id: auctionTeamIds.get(legacyMatch.team1_id),
			right_team_id: auctionTeamIds.get(legacyMatch.team2_id),
			status,
			winner_team_id: status === 'COMPLETED' ? winningTeamId : null,
			revision: 0,
			lineups_locked_at: lockAt,
			lineups_locked_by: lockBy,
			started_at: legacyMatch.started_at == null ? null : isoFromUnix(legacyMatch.started_at),
			ended_at: ['COMPLETED', 'CANCELLED'].includes(status)
				? isoFromUnix(
						legacyMatch.ended_at ?? legacyMatch.deleted_at ?? legacy.ended_at ?? legacy.deleted_at
					)
				: null,
			updated_by: null,
			updated_at: isoFromUnix(
				maxUnix(
					legacyMatch.ended_at,
					legacyMatch.deleted_at,
					legacyMatch.started_at,
					legacy.started_at
				)
			),
			deleted_at: null,
			hard_fearless: false,
			lineup_lock_invariant_version: 1
		});

		if (lockRequired && payload) {
			const seenUsers = new Set();
			for (const legacyTeamId of [legacyMatch.team1_id, legacyMatch.team2_id]) {
				for (const lane of LANES) {
					const userId = payload.assignments[String(legacyTeamId)]?.[lane];
					ensure(
						userId && !seenUsers.has(userId),
						`invalid auction lineup: ${legacyMatch.id}:${lane}`
					);
					seenUsers.add(userId);
					const teamMemberId = auctionMemberIds.get(`${legacyTeamId}:${userId}`);
					ensure(
						teamMemberId,
						`auction lineup user is not an active team member: ${legacyMatch.id}:${userId}`
					);
					tables.auction_match_lineups.push({
						guild_id: guildId,
						tournament_id: id,
						match_id: matchId,
						team_id: auctionTeamIds.get(legacyTeamId),
						lane,
						team_member_id: teamMemberId,
						user_id: userId
					});
				}
			}
		}
	}

	if (format === 'TEN_PERSON' && matches[0]) {
		for (const [slotKey, legacyTeamId] of [
			['A_0', matches[0].team1_id],
			['A_1', matches[0].team2_id]
		]) {
			tables.auction_bracket_slots.push({
				guild_id: guildId,
				tournament_id: id,
				slot_key: slotKey,
				team_id: auctionTeamIds.get(legacyTeamId),
				updated_by: null,
				updated_at: isoFromUnix(matches[0].started_at ?? legacy.started_at)
			});
		}
	}
}

for (const legacyGame of source.games.filter((row) => row.auction_match_id != null)) {
	const matchId = auctionMatchIds.get(legacyGame.auction_match_id);
	if (!matchId) continue;
	const legacyMatch = source.auction_matches.find((row) => row.id === legacyGame.auction_match_id);
	const legacyTournament = allTournamentById.get(legacyMatch.tournament_id);
	appendConfirmedGame(legacyGame, {
		kind: 'auction',
		id: matchId,
		seasonId: seasonIds.get(legacyTournament.season_id)
	});
}

const targetIdMaps = {
	season: seasonIds,
	recruitment: recruitmentIds,
	series: seriesIds,
	game: gameIds,
	'auction-recruitment': auctionRecruitmentIds,
	'auction-tournament': auctionTournamentIds,
	'auction-match': auctionMatchIds
};
for (const legacy of source.admin_audit_log) {
	let payload = null;
	if (legacy.payload != null && legacy.payload !== '') {
		try {
			payload = redactSecrets(JSON.parse(legacy.payload));
		} catch {
			reportWarnings.push({
				reasonCode: 'INVALID_AUDIT_PAYLOAD_OMITTED',
				entity: 'ADMIN_AUDIT',
				legacyId: String(legacy.id)
			});
		}
	}
	const mappedTarget = targetIdMaps[legacy.target_type]?.get(Number(legacy.target_id));
	tables.admin_audit_log.push({
		guild_id: guildId,
		id: uuidV7('admin-audit', legacy.id, legacy.created_at),
		principal_type: 'USER',
		principal_id: legacy.operator_id,
		action: legacy.action,
		target_type: legacy.target_type,
		target_id: mappedTarget ?? legacy.target_id,
		checkpoint_id: null,
		request_id: null,
		before_summary: null,
		after_summary: payload == null ? null : { legacyPayload: payload },
		result: 'APPLIED',
		note: legacy.note,
		created_at: isoFromUnix(legacy.created_at)
	});
}

for (const [counterKey, rows] of [
	['RECRUITMENT', normalizedRecruitments],
	['SERIES', allSeries],
	['AUCTION_RECRUITMENT', normalizedAuctionRecruitments],
	['AUCTION_TOURNAMENT', allTournaments]
]) {
	const maximum = rows.reduce((value, row) => Math.max(value, Number(row.id)), 0);
	tables.guild_counters.push({
		guild_id: guildId,
		counter_key: counterKey,
		next_value: maximum + 1
	});
}

ensure(tables.seasons.length === 1, 'archive history must be unified into one season');
ensure(
	tables.recruitments.length === allSeries.length &&
		tables.recruitments.every((row) => row.status === 'CONVERTED'),
	'only completed-series recruitment parents may remain in the archive'
);
ensure(
	tables.series.length === allSeries.length &&
		tables.series.every((row) => row.status === 'COMPLETED'),
	'a non-completed series remained in the archive'
);
const retainedSourceGames = source.games.filter((row) => {
	if (row.ranked_series_id != null) return retainedSeriesIds.has(row.ranked_series_id);
	if (row.auction_match_id != null) return retainedAuctionMatchIds.has(row.auction_match_id);
	return false;
});
const retainedSourceGameIds = new Set(retainedSourceGames.map((row) => row.id));
ensure(tables.games.length === retainedSourceGames.length, 'a completed-history game was omitted');
ensure(
	tables.game_participants.length === retainedSourceGames.length * 10,
	'a completed-history game roster was not fully reconstructed'
);
ensure(
	tables.game_inputs.length === tables.games.length &&
		tables.game_inputs.every((row) => row.status === 'SUBMITTED'),
	'an unfinished game input remained in the archive'
);
ensure(
	tables.auction_recruitments.length === allTournaments.length &&
		tables.auction_recruitments.every((row) => row.status === 'CONVERTED'),
	'only completed-tournament recruitment parents may remain in the archive'
);
ensure(
	tables.auction_tournaments.length === allTournaments.length &&
		tables.auction_tournaments.every((row) => row.status === 'COMPLETED'),
	'a non-completed auction tournament remained in the archive'
);
ensure(
	tables.auction_matches.length === retainedAuctionMatches.length &&
		tables.auction_matches.every((row) => row.status === 'COMPLETED'),
	'a non-completed auction match remained in the archive'
);
const unifiedSeasonId = tables.seasons[0].id;
for (const tableName of [
	'recruitments',
	'series',
	'auction_recruitments',
	'auction_tournaments',
	'auction_matches',
	'games',
	'user_lane_mmr',
	'mmr_ledger_entries'
]) {
	ensure(
		tables[tableName].every((row) => row.season_id === unifiedSeasonId),
		`${tableName} is not unified into the archive season`
	);
}

const sourceSeriesById = indexBy(source.series, 'id');
const sourceAuctionMatchesById = indexBy(source.auction_matches, 'id');
const sourceAuctionTournamentsById = indexBy(source.auction_tournaments, 'id');
function sourceGameWasSoftDeleted(game) {
	if (game.ranked_series_id != null) {
		return sourceSeriesById.get(game.ranked_series_id)?.deleted_at != null;
	}
	const legacyMatch = sourceAuctionMatchesById.get(game.auction_match_id);
	const legacyTournament = sourceAuctionTournamentsById.get(legacyMatch?.tournament_id);
	return legacyMatch?.deleted_at != null || legacyTournament?.deleted_at != null;
}
const includedSoftDeleted = {
	series: allSeries.filter((row) => row.deleted_at != null).length,
	tournaments: allTournaments.filter((row) => row.deleted_at != null).length,
	games: retainedSourceGames.filter(sourceGameWasSoftDeleted).length
};
const omittedSoftDeleted = {
	series: source.series.filter((row) => row.deleted_at != null && !retainedSeriesIds.has(row.id))
		.length,
	tournaments: source.auction_tournaments.filter(
		(row) => row.deleted_at != null && !retainedAuctionTournamentIds.has(row.id)
	).length,
	games: source.games.filter(
		(row) => sourceGameWasSoftDeleted(row) && !retainedSourceGameIds.has(row.id)
	).length
};
const reconciledSoftDeleted = Object.fromEntries(
	Object.keys(includedSoftDeleted).map((key) => [
		key,
		includedSoftDeleted[key] + omittedSoftDeleted[key]
	])
);
ensure(
	canonicalJson(reconciledSoftDeleted) === canonicalJson(analysis.source.omittedSoftDeleted),
	'soft-deleted source reconciliation changed after analysis'
);

for (const name of TABLE_NAMES) {
	tables[name].sort((left, right) => compareText(JSON.stringify(left), JSON.stringify(right)));
}

const archive = {
	format: 'MONKEYBOT_DATA_TRANSFER',
	kind: 'GUILD_FULL_SNAPSHOT',
	schemaVersion: 1,
	exportedAt: freezeIso,
	source: { guildId, guildName },
	tables
};
const archiveJson = `${JSON.stringify(archive, null, 2)}\n`;
const archiveBytes = Buffer.byteLength(archiveJson);
const totalRows = Object.values(tables).reduce((sum, rows) => sum + rows.length, 0);
ensure(totalRows <= MAX_ARCHIVE_ROWS, `archive exceeds ${MAX_ARCHIVE_ROWS} rows`);
ensure(archiveBytes <= MAX_ARCHIVE_BYTES, `archive exceeds ${MAX_ARCHIVE_BYTES} bytes`);

const report = {
	format: 'MOOKBOT_V3_ARCHIVE_CONVERSION_REPORT',
	schemaVersion: 1,
	createdAt: new Date().toISOString(),
	writeFreezeAt: freezeIso,
	runId,
	importExecuted: false,
	source: {
		manifestFormat: manifest.format,
		schemaFingerprint: manifest.schemaFingerprint,
		snapshotChecksum: manifest.snapshotChecksum,
		analysisChecksum,
		valkeyChecksum,
		championCatalogVersion: catalog.version,
		championCatalogChecksum,
		stableDoubleRead: manifest.validation.stableDoubleRead,
		foreignKeyViolationCount: manifest.validation.foreignKeyViolations.length,
		limitations: manifest.limitations ?? []
	},
	target: {
		guildId,
		guildName,
		archiveKind: archive.kind,
		archiveSchemaVersion: archive.schemaVersion
	},
	output: {
		archivePath: outputPath,
		archiveSha256: hashHex(archiveJson),
		archiveBytes,
		totalRows,
		tableCounts: Object.fromEntries(TABLE_NAMES.map((name) => [name, tables[name].length]))
	},
	reconciliation: {
		singleSeasonHistory: {
			sourceSeasonCount: source.seasons.length,
			archiveSeasonCount: tables.seasons.length,
			strategy: 'ALL_HISTORY_IN_SINGLE_ACTIVE_SEASON'
		},
		omittedUnfinishedRecruitments: omittedUnfinishedRecruitmentSummary,
		omittedIncompleteSeries: omittedIncompleteSeriesSummary,
		omittedIncompleteAuctionWorkflows: omittedIncompleteAuctionWorkflowSummary,
		inferredBaselineSeriesMmr: {
			participantCount: inferredBaselineSeriesMmrCount,
			mmr: DEFAULT_MMR,
			strategy: 'DEFAULT_MMR_WHEN_SOURCE_SNAPSHOT_IS_UNAVAILABLE'
		},
		includedSoftDeleted,
		omittedSoftDeleted,
		info: reportInfo,
		warnings: reportWarnings,
		acceptedReentryCount: reportWarnings.filter((row) => row.reasonCode === 'ACCEPTED_REENTRY')
			.length
	},
	validation: {
		analysisErrorCount: analysis.validation.errorCount,
		analysisWarningCount: analysis.validation.warningCount,
		archiveTableSetExact: Object.keys(tables).length === TABLE_NAMES.length,
		withinRowLimit: totalRows <= MAX_ARCHIVE_ROWS,
		withinByteLimit: archiveBytes <= MAX_ARCHIVE_BYTES,
		readyForApiPreview: true,
		actualDatabaseImportPerformed: false
	}
};

fs.mkdirSync(path.dirname(outputPath), { recursive: true, mode: 0o700 });
fs.mkdirSync(path.dirname(reportPath), { recursive: true, mode: 0o700 });
fs.writeFileSync(outputPath, archiveJson, { mode: 0o600 });
fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600 });

console.log(
	JSON.stringify(
		{
			archivePath: outputPath,
			reportPath,
			archiveSha256: report.output.archiveSha256,
			archiveBytes,
			totalRows,
			acceptedReentryCount: report.reconciliation.acceptedReentryCount,
			readyForApiPreview: true,
			actualDatabaseImportPerformed: false
		},
		null,
		2
	)
);
