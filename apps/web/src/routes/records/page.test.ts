import { describe, expect, it, vi } from 'vitest';
import type { RiotRecords } from '$lib/contracts/riotRecords';
import { load } from './+page';

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

const baseData = {
	query: 'Longcat#KR1',
	lookup: { gameName: 'Longcat', tagLine: 'KR1', platform: 'KR' as const },
	loadError: null
};

describe('/records universal load', () => {
	it('does not load Data Dragon without valid records', async () => {
		const fetcher = vi.fn();
		const result = await load({
			data: { ...baseData, records: null },
			fetch: fetcher
		} as never);

		expect(result).toEqual({ ...baseData, records: null, championCatalog: null });
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('loads the champion catalog only after records resolve', async () => {
		const fetcher = vi.fn(async (input: RequestInfo | URL) => {
			const path = String(input);
			if (path === '/dd/api/versions.json') return Response.json(['16.13.1']);
			if (path === '/dd/cdn/16.13.1/data/ko_KR/champion.json') {
				return Response.json({
					type: 'champion',
					format: 'standAloneComplex',
					version: '16.13.1',
					data: {
						LeeSin: {
							version: '16.13.1',
							id: 'LeeSin',
							key: '64',
							name: '리 신',
							title: '눈먼 수도승',
							image: { full: 'LeeSin.png' }
						}
					}
				});
			}
			throw new Error(`unexpected request: ${path}`);
		});

		const result = await load({
			data: { ...baseData, records },
			fetch: fetcher
		} as never);

		expect(fetcher).toHaveBeenCalledTimes(2);
		expect(result?.records).toEqual(records);
		expect(result?.championCatalog).toMatchObject({
			version: '16.13.1',
			locale: 'ko_KR'
		});
		expect(result?.championCatalog?.byId.LeeSin.name).toBe('리 신');
	});
});
