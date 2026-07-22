import { afterEach, describe, expect, it, vi } from 'vitest';
import type { RiotRecords, RiotRecordsLookup } from '$lib/contracts/riotRecords';
import {
	fetchMoreRiotMatches,
	fetchRiotRecords,
	parseRiotRecordsQuery,
	refreshRiotRecords,
	riotRecordsErrorText
} from './riotRecords';

const requestId = '01900000-0000-7000-8000-000000000001';
const lookup: RiotRecordsLookup = { gameName: 'Long Cat', tagLine: 'KR1', platform: 'KR' };
const records: RiotRecords = {
	account: { puuid: 'puuid-main', gameName: 'Long Cat', tagLine: 'KR1' },
	platform: 'KR',
	routingRegion: 'ASIA',
	profileIconId: null,
	summonerLevel: null,
	ranks: [],
	championAggregates: [],
	recentMatches: [],
	sample: {
		recentMatches: { requested: 10, returned: 0 },
		championAggregates: {
			seasonStartUnixSeconds: 1_767_841_200,
			discoveredMatches: 0,
			analyzedMatches: 0,
			truncated: false
		}
	},
	refreshedAt: '2026-07-13T00:00:00Z',
	cache: { state: 'fresh' }
};

function success(snapshot: RiotRecords = records): Response {
	return Response.json({ data: snapshot, meta: { requestId } });
}

describe('Riot records API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('parses exactly one GameName#TagLine separator and defaults to KR', () => {
		expect(parseRiotRecordsQuery('  Long Cat # KR1  ')).toEqual(lookup);
		expect(parseRiotRecordsQuery('Longcat')).toBeNull();
		expect(parseRiotRecordsQuery('#KR1')).toBeNull();
		expect(parseRiotRecordsQuery('Longcat#')).toBeNull();
		expect(parseRiotRecordsQuery('Long#cat#KR1')).toBeNull();
	});

	it('gets public records through the same-origin proxy', async () => {
		const fetcher = vi.fn(async () => success());

		await expect(fetchRiotRecords(fetcher, lookup)).resolves.toEqual(records);
		expect(fetcher).toHaveBeenCalledWith(
			'/api/riot/records?gameName=Long+Cat&tagLine=KR1&platform=KR',
			{
				method: 'GET',
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
	});

	it('refreshes with session credentials, CSRF, and the same strict snapshot', async () => {
		const refreshed = { ...records, cache: { state: 'refreshed' as const } };
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return success(refreshed);
			}
		);
		vi.stubGlobal('document', { cookie: 'monkeybot_csrf=csrf-token' });
		vi.stubGlobal('fetch', fetcher);

		await expect(refreshRiotRecords(lookup)).resolves.toEqual(refreshed);
		expect(fetcher).toHaveBeenCalledOnce();
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe('/api/riot/records/refresh');
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('X-CSRF-Token')).toBe('csrf-token');
		expect(new Headers(init?.headers).get('content-type')).toBe('application/json');
		expect(JSON.parse(String(init?.body))).toEqual(lookup);
	});

	it('loads the next recent-match page through the public same-origin route', async () => {
		const page = {
			account: records.account,
			platform: 'KR' as const,
			routingRegion: 'ASIA' as const,
			targetPuuid: records.account.puuid,
			matches: [],
			page: { offset: 10, requested: 10, returned: 0, hasMore: false, nextOffset: null }
		};
		const fetcher = vi.fn(async () => Response.json({ data: page, meta: { requestId } }));
		vi.stubGlobal('fetch', fetcher);

		await expect(fetchMoreRiotMatches(lookup, 10, 10)).resolves.toEqual(page);
		expect(fetcher).toHaveBeenCalledWith(
			'/api/riot/records/matches?gameName=Long+Cat&tagLine=KR1&platform=KR&offset=10&count=10',
			{
				method: 'GET',
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
	});

	it('normalizes backend, invalid-response, and missing-CSRF failures', async () => {
		const notFound = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'riot_account_not_found',
						message: '계정을 찾을 수 없습니다.',
						requestId
					}
				},
				{ status: 404 }
			)
		);
		const notFoundError = await fetchRiotRecords(notFound, lookup).catch((cause) => cause);
		expect(notFoundError).toMatchObject({
			status: 404,
			code: 'riot_account_not_found',
			message: '계정을 찾을 수 없습니다.',
			requestId
		});
		expect(riotRecordsErrorText(notFoundError)).toBe(
			`계정을 찾을 수 없습니다. (요청 ID: ${requestId})`
		);

		const invalid = vi.fn(async () => Response.json({ data: {}, meta: { requestId } }));
		await expect(fetchRiotRecords(invalid, lookup)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});

		vi.stubGlobal('document', { cookie: '' });
		vi.stubGlobal('fetch', vi.fn());
		await expect(refreshRiotRecords(lookup)).rejects.toMatchObject({
			status: 0,
			code: 'csrf_token_missing'
		});
	});
});
