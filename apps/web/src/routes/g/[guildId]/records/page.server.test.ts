import { describe, expect, it, vi } from 'vitest';
import type { GuildRecordsSearchSnapshot } from '$lib/contracts/workflow';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const guildId = '1525851181157580861';
const seasonId = '01900000-0000-7000-8000-000000000010';
const snapshot: GuildRecordsSearchSnapshot = {
	member: {
		displayName: 'Longcat',
		avatarUrl: 'https://cdn.discordapp.com/avatars/1/avatar.png',
		mainLane: 'MID',
		profileHref: '/users/390526792496054283'
	},
	championPreferences: {
		TOP: ['Ornn'],
		JUNGLE: [],
		MID: ['Ahri', 'Orianna'],
		BOTTOM: ['Ezreal'],
		SUPPORT: []
	},
	riotAccounts: [
		{
			platform: 'KR',
			gameName: 'Longcat',
			tagLine: 'KR1',
			profileIconId: 588,
			isMain: true
		}
	],
	seasons: [
		{
			id: seasonId,
			name: '시즌 1',
			status: 'ACTIVE',
			startedAt: '2026-07-13T00:00:00Z',
			endedAt: null
		}
	],
	selectedSeasonId: seasonId,
	summary: { games: 0, wins: 0, losses: 0 },
	laneMmr: [
		{ lane: 'TOP', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'JUNGLE', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'MID', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'BOTTOM', mmr: 1500, games: 0, wins: 0 },
		{ lane: 'SUPPORT', mmr: 1500, games: 0, wins: 0 }
	],
	championRecords: [],
	records: [],
	nextCursor: null
};

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

function run(url: string, fetcher = vi.fn()) {
	return load({
		fetch: fetcher,
		params: { guildId },
		parent: vi.fn().mockResolvedValue({ guildId }),
		url: new URL(url)
	} as never);
}

describe('guild records server load', () => {
	it('returns a search-first empty state without calling the API', async () => {
		const fetcher = vi.fn();

		await expect(run(`http://localhost:5173/g/${guildId}/records`, fetcher)).resolves.toEqual({
			query: '',
			seasonId: null,
			snapshot: null,
			loadError: null
		});
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('SSR-loads the authoritative snapshot and selected season', async () => {
		const fetcher = vi.fn(async () => envelope(snapshot));

		await expect(
			run(`http://localhost:5173/g/${guildId}/records?q=Longcat&seasonId=${seasonId}`, fetcher)
		).resolves.toEqual({ query: 'Longcat', seasonId, snapshot, loadError: null });
		expect(fetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/records/search?q=Longcat&limit=20&seasonId=${seasonId}`,
			{
				method: 'GET',
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
	});

	it('keeps a backend resolution error in the records page state', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'ambiguous_guild_member',
						message: '같은 이름의 길드 멤버가 여러 명입니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);

		await expect(
			run(`http://localhost:5173/g/${guildId}/records?q=Twin`, fetcher)
		).resolves.toEqual({
			query: 'Twin',
			seasonId: null,
			snapshot: null,
			loadError: `같은 이름의 길드 멤버가 여러 명입니다. (요청 ID: ${requestId})`
		});
	});

	it('shows strict response drift as a load error instead of rendering mock data', async () => {
		const fetcher = vi.fn(async () => envelope({ ...snapshot, hiddenRiotIds: 1 }));

		await expect(
			run(`http://localhost:5173/g/${guildId}/records?q=Longcat`, fetcher)
		).resolves.toMatchObject({
			snapshot: null,
			loadError: '길드 전적 응답 형식이 올바르지 않습니다.'
		});
	});
});
