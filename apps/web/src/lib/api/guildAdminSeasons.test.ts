import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GuildAdminSeasonsSnapshot } from '$lib/contracts/workflow';
import { fetchGuildAdminSeasons, startGuildAdminSeason } from './guildAdminSeasons';

const guildId = '1525851181157580861';
const currentSeasonId = '01900000-0000-7000-8000-000000000010';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
const snapshot: GuildAdminSeasonsSnapshot = {
	current: {
		id: currentSeasonId,
		name: '시즌 1',
		status: 'ACTIVE',
		startedAt: '2026-07-13T01:00:00Z',
		endedAt: null,
		createdBy: null,
		participantCount: 0,
		generalSeriesCount: 0,
		auctionTournamentCount: 0,
		auctionTenCount: 0,
		auctionTwentyCount: 0,
		confirmedGameCount: 0,
		lastGameAt: null
	},
	completed: []
};

function envelope(data: GuildAdminSeasonsSnapshot = snapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

function fetchMock(response: () => Response) {
	return vi.fn(async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
		void args;
		return response();
	});
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('guild admin seasons API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads a strict authoritative season snapshot', async () => {
		const fetcher = fetchMock(() => envelope());
		installBrowser(fetcher);

		await expect(fetchGuildAdminSeasons(guildId)).resolves.toEqual(snapshot);
		expect(fetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/seasons`,
			expect.objectContaining({ credentials: 'same-origin' })
		);
		expect(new Headers(fetcher.mock.calls[0]![1]?.headers).get('accept')).toBe('application/json');
	});

	it('posts only the season name and expected current id with CSRF and idempotency', async () => {
		const authoritative: GuildAdminSeasonsSnapshot = {
			current: {
				...snapshot.current,
				id: '01900000-0000-7000-8000-000000000011',
				name: '시즌 2'
			},
			completed: [
				{
					...snapshot.current,
					status: 'CLOSED',
					endedAt: '2026-07-13T02:00:00Z'
				}
			]
		};
		const fetcher = fetchMock(() => envelope(authoritative));
		installBrowser(fetcher, 'other=value; monkeybot_csrf=csrf-token');

		await expect(
			startGuildAdminSeason(guildId, {
				name: '시즌 2',
				expectedCurrentSeasonId: currentSeasonId,
				idempotencyKey
			})
		).resolves.toEqual(authoritative);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/seasons`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		const headers = new Headers(init?.headers);
		expect(headers.get('accept')).toBe('application/json');
		expect(headers.get('content-type')).toBe('application/json');
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
		expect(JSON.parse(String(init?.body))).toEqual({
			name: '시즌 2',
			expectedCurrentSeasonId: currentSeasonId
		});
	});

	it('preserves a current season conflict and rejects non-strict success data', async () => {
		const conflict = fetchMock(() =>
			Response.json(
				{
					error: {
						code: 'current_season_conflict',
						message: '현재 시즌이 이미 변경되었습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(conflict);
		await expect(
			startGuildAdminSeason(guildId, {
				name: '시즌 2',
				expectedCurrentSeasonId: currentSeasonId,
				idempotencyKey
			})
		).rejects.toMatchObject({ status: 409, code: 'current_season_conflict' });

		const malformed = fetchMock(() =>
			Response.json({
				data: { ...snapshot, unexpected: true },
				meta: { requestId }
			})
		);
		vi.stubGlobal('fetch', malformed);
		await expect(fetchGuildAdminSeasons(guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('fails before mutation when CSRF or idempotency metadata is missing', async () => {
		const fetcher = fetchMock(() => envelope());
		installBrowser(fetcher, 'other=value');
		await expect(
			startGuildAdminSeason(guildId, {
				name: '시즌 2',
				expectedCurrentSeasonId: currentSeasonId,
				idempotencyKey
			})
		).rejects.toMatchObject({ status: 0, code: 'csrf_token_missing' });

		vi.stubGlobal('document', { cookie: 'monkeybot_csrf=csrf-token' });
		await expect(
			startGuildAdminSeason(guildId, {
				name: '시즌 2',
				expectedCurrentSeasonId: currentSeasonId,
				idempotencyKey: 'not-a-uuid'
			})
		).rejects.toMatchObject({ status: 0, code: 'invalid_idempotency_key' });
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('normalizes network failures without exposing transport details', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			throw new Error('connect ECONNREFUSED password=secret');
		});
		installBrowser(fetcher);

		await expect(fetchGuildAdminSeasons(guildId)).rejects.toMatchObject({
			message: 'API 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
