import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
	GuildFullDataTransferApplyResult,
	GuildFullDataTransferExportData,
	GuildFullDataTransferPreview,
	GuildFullSnapshotArchive
} from '$lib/contracts/guildFullDataTransfer';
import {
	applyGuildFullSnapshotImport,
	exportGuildFullSnapshot,
	previewGuildFullSnapshotImport,
	serializeGuildFullSnapshotArchive
} from './guildDataTransfer';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const importId = '01900000-0000-7000-8000-000000000002';
const digest = 'a'.repeat(64);
const targetDigest = 'b'.repeat(64);
const idempotencyKey = '01900000-0000-7000-8000-000000000003';

const archive: GuildFullSnapshotArchive = {
	format: 'MONKEYBOT_DATA_TRANSFER',
	kind: 'GUILD_FULL_SNAPSHOT',
	schemaVersion: 1,
	exportedAt: '2026-07-16T06:00:00Z',
	source: { guildId, guildName: 'MonkeyBot Test' },
	tables: {
		users: [],
		user_discord_identities: [],
		user_privacy_settings: [],
		user_champion_preferences: [],
		riot_accounts: [],
		dummy_user_profiles: [],
		guild_members: [],
		user_guild_preferences: [],
		guild_counters: [],
		seasons: [],
		recruitments: [],
		recruitment_participants: [],
		recruitment_preferred_lanes: [],
		recruitment_entry_drafts: [],
		recruitment_entry_assignments: [],
		series: [],
		series_participants: [],
		auction_recruitments: [],
		auction_recruitment_participants: [],
		auction_setup_drafts: [],
		auction_setup_teams: [],
		auction_tournaments: [],
		auction_teams: [],
		auction_bid_candidates: [],
		auction_lots: [],
		auction_actions: [],
		auction_team_members: [],
		auction_point_ledger: [],
		auction_bracket_slots: [],
		auction_matches: [],
		auction_match_lineups: [],
		game_inputs: [],
		game_input_picks: [],
		game_input_bans: [],
		games: [{ guild_id: guildId, id: '01900000-0000-7000-8000-000000000004' }],
		game_participants: [],
		game_participant_stats: [],
		game_bans: [],
		user_lane_mmr: [],
		mmr_ledger_entries: [],
		admin_audit_log: []
	}
};

const summary = {
	totalRows: 1,
	users: 0,
	members: 0,
	seasons: 0,
	recruitments: 0,
	series: 0,
	games: 1,
	auctionTournaments: 0,
	auctionMatches: 0,
	mmrLedgerEntries: 0,
	auditEntries: 0
};

const exportData: GuildFullDataTransferExportData = {
	archive,
	fileName: 'monkeybot-full-test-v1.json',
	summary
};

const preview: GuildFullDataTransferPreview = {
	importId,
	digest,
	targetDigest,
	expiresAt: '2026-07-16T06:15:00Z',
	source: archive.source,
	summary,
	conflicts: [],
	remapsSourceGuild: true,
	usesTargetActiveSeason: true,
	rebasesDisplayNumbers: true
};

const applied: GuildFullDataTransferApplyResult = {
	importId,
	status: 'APPLIED',
	appliedAt: '2026-07-16T06:01:00Z',
	summary,
	remappedSourceGuild: true,
	usedTargetActiveSeason: true,
	rebasedDisplayNumbers: true
};

function response(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch) {
	vi.stubGlobal('document', { cookie: 'monkeybot_csrf=csrf-token' });
	vi.stubGlobal('fetch', fetcher);
}

describe('guild full data transfer API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('exports, previews, and applies the exact archive and target digests', async () => {
		const fetcher = vi
			.fn()
			.mockResolvedValueOnce(response(exportData))
			.mockResolvedValueOnce(response(preview))
			.mockResolvedValueOnce(response(applied));
		installBrowser(fetcher);

		await expect(exportGuildFullSnapshot(guildId)).resolves.toEqual(exportData);
		await expect(previewGuildFullSnapshotImport(guildId, archive)).resolves.toEqual(preview);
		await expect(
			applyGuildFullSnapshotImport(guildId, {
				importId,
				expectedDigest: digest,
				expectedTargetDigest: targetDigest,
				idempotencyKey
			})
		).resolves.toEqual(applied);

		expect(fetcher.mock.calls[0]?.[0]).toBe(`/api/guilds/${guildId}/full-data-export`);
		expect(fetcher.mock.calls[1]?.[0]).toBe(`/api/guilds/${guildId}/full-data-imports/preview`);
		expect(fetcher.mock.calls[2]?.[0]).toBe(
			`/api/guilds/${guildId}/full-data-imports/${importId}/apply`
		);
		const applyInit = fetcher.mock.calls[2]?.[1];
		expect(new Headers(applyInit?.headers).get('Idempotency-Key')).toBe(idempotencyKey);
		expect(JSON.parse(String(applyInit?.body))).toEqual({
			expectedDigest: digest,
			expectedTargetDigest: targetDigest
		});
	});

	it('serializes the full relational history as readable JSON', () => {
		const serialized = serializeGuildFullSnapshotArchive(archive);
		expect(serialized).toContain('"kind": "GUILD_FULL_SNAPSHOT"');
		expect(serialized).toContain('"games"');
		expect(serialized.endsWith('\n')).toBe(true);
		expect(JSON.parse(serialized)).toEqual(archive);
	});
});
