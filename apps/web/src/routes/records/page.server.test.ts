import { describe, expect, it, vi } from 'vitest';
import type { RiotRecords } from '$lib/contracts/riotRecords';
import { load } from './+page.server';

const requestId = '01900000-0000-7000-8000-000000000001';
const records: RiotRecords = {
	account: { puuid: 'puuid-main', gameName: 'Longcat', tagLine: 'KR1' },
	platform: 'KR',
	routingRegion: 'ASIA',
	profileIconId: 12,
	summonerLevel: 300,
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

function envelope(data: RiotRecords): Response {
	return Response.json({ data, meta: { requestId } });
}

async function run(url: string, fetcher = vi.fn()) {
	return load({ fetch: fetcher, url: new URL(url) } as never);
}

describe('/records server load', () => {
	it('does not call the API without a query', async () => {
		const fetcher = vi.fn();
		await expect(run('http://localhost:5173/records', fetcher)).resolves.toEqual({
			query: '',
			lookup: null,
			records: null,
			loadError: null
		});
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('returns a user-facing state for an invalid query without calling the API', async () => {
		const fetcher = vi.fn();
		await expect(run('http://localhost:5173/records?q=Longcat', fetcher)).resolves.toEqual({
			query: 'Longcat',
			lookup: null,
			records: null,
			loadError: 'Riot ID를 GameName#TagLine 형식으로 입력해 주세요.'
		});
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('loads a valid Riot ID through the public same-origin route', async () => {
		const fetcher = vi.fn(async () => envelope(records));
		const result = await run('http://localhost:5173/records?q=Longcat%23KR1', fetcher);

		expect(result).toEqual({
			query: 'Longcat#KR1',
			lookup: { gameName: 'Longcat', tagLine: 'KR1', platform: 'KR' },
			records,
			loadError: null
		});
		expect(fetcher).toHaveBeenCalledWith(
			'/api/riot/records?gameName=Longcat&tagLine=KR1&platform=KR',
			{
				method: 'GET',
				credentials: 'same-origin',
				headers: { accept: 'application/json' }
			}
		);
	});

	it.each([
		[404, 'riot_account_not_found', '요청한 Riot 계정을 찾을 수 없습니다.'],
		[503, 'riot_unavailable', 'Riot 전적 서비스를 일시적으로 사용할 수 없습니다.']
	])('returns an error state for API status %s', async (status, code, expectedMessage) => {
		const fetcher = vi.fn(async () =>
			Response.json({ error: { code, message: expectedMessage, requestId } }, { status })
		);
		const result = await run('http://localhost:5173/records?q=Longcat%23KR1', fetcher);

		expect(result).toMatchObject({
			lookup: { gameName: 'Longcat', tagLine: 'KR1', platform: 'KR' },
			records: null,
			loadError: `${expectedMessage} (요청 ID: ${requestId})`
		});
	});
});
