import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
	GuildDataTransferApplyResult,
	GuildDataTransferExportData,
	GuildDataTransferPreview,
	MemberSeasonMmrSeedArchive
} from '$lib/contracts/guildDataTransfer';
import {
	applyGuildMemberSeasonMmrSeedImport,
	exportGuildMemberSeasonMmrSeed,
	previewGuildMemberSeasonMmrSeedImport,
	serializeGuildMemberSeasonMmrSeedArchive
} from './guildDataTransfer';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const importId = '01900000-0000-7000-8000-000000000002';
const seasonId = '01900000-0000-7000-8000-000000000003';
const idempotencyKey = '01900000-0000-7000-8000-000000000004';
const digest = 'a'.repeat(64);

const archive: MemberSeasonMmrSeedArchive = {
	format: 'MONKEYBOT_DATA_TRANSFER',
	schemaVersion: 1,
	kind: 'MEMBER_SEASON_MMR_SEED',
	exportedAt: '2026-07-15T06:00:00Z',
	source: {
		guildId,
		guildName: 'MonkeyBot Test',
		season: { id: seasonId, name: '2026 시즌 1' }
	},
	members: [
		{
			discordId: '390526792496054283',
			displayName: 'Longcat',
			lanes: [
				{ lane: 'TOP', mmr: 1500 },
				{ lane: 'JUNGLE', mmr: 1500 },
				{ lane: 'MID', mmr: 1540 },
				{ lane: 'BOTTOM', mmr: 1500 },
				{ lane: 'SUPPORT', mmr: 1500 }
			]
		}
	]
};

const exportData: GuildDataTransferExportData = {
	archive,
	fileName: 'monkeybot-mmr-seed.json'
};

const preview: GuildDataTransferPreview = {
	importId,
	digest,
	expiresAt: '2026-07-15T06:10:00Z',
	source: archive.source,
	summary: { archiveMembers: 1, matchedMembers: 1, laneEntries: 5, changes: 1 },
	conflicts: [],
	targetRevision: 7
};

const applied: GuildDataTransferApplyResult = {
	importId,
	status: 'APPLIED',
	appliedAt: '2026-07-15T06:01:00Z',
	summary: preview.summary,
	targetRevision: 8
};

function response(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('guild data transfer API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('exports the MMR seed with a CSRF-protected POST', async () => {
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return response(exportData);
			}
		);
		installBrowser(fetcher);

		await expect(exportGuildMemberSeasonMmrSeed(guildId)).resolves.toEqual(exportData);
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/data-export`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		const headers = new Headers(init?.headers);
		expect(headers.get('accept')).toBe('application/json');
		expect(headers.get('content-type')).toBe('application/json');
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
	});

	it('serializes the archive as readable plain JSON without an opaque container', () => {
		const serialized = serializeGuildMemberSeasonMmrSeedArchive(archive);

		expect(serialized).toContain('\n  "format": "MONKEYBOT_DATA_TRANSFER"');
		expect(serialized).toContain('"displayName": "Longcat"');
		expect(serialized.endsWith('\n')).toBe(true);
		expect(JSON.parse(serialized)).toEqual(archive);
	});

	it('rejects an export response whose file name is not a single JSON file', async () => {
		const fetcher = vi.fn(async () => response({ ...exportData, fileName: '../archive.txt' }));
		installBrowser(fetcher);

		await expect(exportGuildMemberSeasonMmrSeed(guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response',
			requestId
		});
	});

	it('previews the selected document then applies its exact digest and target revision', async () => {
		const fetcher = vi
			.fn()
			.mockResolvedValueOnce(response(preview))
			.mockResolvedValueOnce(response(applied));
		installBrowser(fetcher);

		await expect(previewGuildMemberSeasonMmrSeedImport(guildId, archive)).resolves.toEqual(preview);
		await expect(
			applyGuildMemberSeasonMmrSeedImport(guildId, {
				importId,
				expectedDigest: digest,
				expectedTargetRevision: 7,
				idempotencyKey
			})
		).resolves.toEqual(applied);

		const [previewPath, previewInit] = fetcher.mock.calls[0]!;
		expect(previewPath).toBe(`/api/guilds/${guildId}/data-imports/preview`);
		expect(previewInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(previewInit?.body))).toEqual({ archive });

		const [applyPath, applyInit] = fetcher.mock.calls[1]!;
		expect(applyPath).toBe(`/api/guilds/${guildId}/data-imports/${importId}/apply`);
		expect(applyInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		const applyHeaders = new Headers(applyInit?.headers);
		expect(applyHeaders.get('X-CSRF-Token')).toBe('csrf-token');
		expect(applyHeaders.get('Idempotency-Key')).toBe(idempotencyKey);
		expect(JSON.parse(String(applyInit?.body))).toEqual({
			expectedDigest: digest,
			expectedTargetRevision: 7
		});
	});

	it('preserves a concrete backend rejection and its request id', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'preview_conflicts',
						message: '활성 길드 멤버와 일치하지 않는 항목이 있습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		await expect(previewGuildMemberSeasonMmrSeedImport(guildId, archive)).rejects.toMatchObject({
			status: 409,
			code: 'preview_conflicts',
			requestId,
			message: '활성 길드 멤버와 일치하지 않는 항목이 있습니다.'
		});
	});

	it('keeps the request id when a successful response violates the data contract', async () => {
		const fetcher = vi.fn(async () => Response.json({ data: {}, meta: { requestId } }));
		installBrowser(fetcher);

		await expect(exportGuildMemberSeasonMmrSeed(guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response',
			requestId
		});
	});

	it('accepts a valid preview containing one conflict per supported archive member', async () => {
		const conflicts = Array.from({ length: 1_000 }, (_, index) => ({
			reason: 'member_not_active',
			message: '대상 길드에서 활성 Discord 멤버를 찾을 수 없습니다.',
			discordId: `9${String(index).padStart(17, '0')}`,
			displayName: `Member ${index + 1}`
		}));
		const fetcher = vi.fn(async () => response({ ...preview, conflicts }));
		installBrowser(fetcher);

		await expect(previewGuildMemberSeasonMmrSeedImport(guildId, archive)).resolves.toMatchObject({
			conflicts: expect.arrayContaining([
				expect.objectContaining({ discordId: conflicts[0]?.discordId })
			])
		});
	});
});
