import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GuildRecordsSearchSnapshot } from '$lib/contracts/workflow';
import { fetchGuildRecords } from './guildRecords';

const guildId = '1525851181157580861';
const seasonId = '01900000-0000-7000-8000-000000000010';
const requestId = '01900000-0000-7000-8000-000000000001';
const snapshot: GuildRecordsSearchSnapshot = {
	member: { displayName: 'Longcat', avatarUrl: null, mainLane: 'MID' },
	championPreferences: {
		TOP: [],
		JUNGLE: [],
		MID: ['Ahri'],
		BOTTOM: [],
		SUPPORT: []
	},
	riotAccounts: [],
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

function envelope(data: unknown = snapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('guild records API client', () => {
	afterEach(() => vi.restoreAllMocks());

	it('loads a strict guild record snapshot with the selected season and cursor', async () => {
		const fetcher = vi.fn(async () => envelope());

		await expect(
			fetchGuildRecords(fetcher, guildId, {
				query: '  Longcat  ',
				seasonId,
				cursor: 'opaque-cursor',
				limit: 20
			})
		).resolves.toEqual(snapshot);
		expect(fetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/records/search?q=Longcat&limit=20&seasonId=${seasonId}&cursor=opaque-cursor`,
			{
				method: 'GET',
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
	});

	it('rejects invalid query metadata before network I/O', async () => {
		const fetcher = vi.fn();

		await expect(fetchGuildRecords(fetcher, guildId, { query: '   ' })).rejects.toMatchObject({
			status: 0,
			code: 'invalid_search_query'
		});
		await expect(
			fetchGuildRecords(fetcher, guildId, { query: 'Longcat', limit: 51 })
		).rejects.toMatchObject({ status: 0, code: 'invalid_search_query' });
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('preserves backend search errors and rejects contract drift', async () => {
		const conflict = vi.fn(async () =>
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
		await expect(fetchGuildRecords(conflict, guildId, { query: 'Twin' })).rejects.toMatchObject({
			status: 409,
			code: 'ambiguous_guild_member'
		});

		const malformed = vi.fn(async () => envelope({ ...snapshot, internalUserId: 'secret' }));
		await expect(fetchGuildRecords(malformed, guildId, { query: 'Longcat' })).rejects.toMatchObject(
			{
				status: 200,
				code: 'invalid_response'
			}
		);
	});

	it('normalizes transport failures without exposing their details', async () => {
		const fetcher = vi.fn(async () => {
			throw new Error('connect ECONNREFUSED password=secret');
		});

		await expect(fetchGuildRecords(fetcher, guildId, { query: 'Longcat' })).rejects.toMatchObject({
			message: '길드 전적 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
