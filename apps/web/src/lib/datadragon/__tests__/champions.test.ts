import { describe, expect, it, vi } from 'vitest';
import {
	championCatalogUrl,
	championIconUrl,
	createChampionCatalog,
	findChampion,
	loadCachedChampionCatalog,
	loadChampionCatalog,
	normalizeChampionSlug,
	searchChampions
} from '../champions';
import type { DataDragonChampionResponse } from '../types';
import fullChampionFixture from '../../../../static/fixtures/datadragon/champion-ko_KR.fixture.json';

const championResponse: DataDragonChampionResponse = {
	type: 'champion',
	format: 'standAloneComplex',
	version: '14.24.1',
	data: {
		LeeSin: {
			version: '14.24.1',
			id: 'LeeSin',
			key: '64',
			name: '리 신',
			title: '눈먼 수도승',
			image: { full: 'LeeSin.png' },
			tags: ['Fighter', 'Assassin']
		},
		Kaisa: {
			version: '14.24.1',
			id: 'Kaisa',
			key: '145',
			name: '카이사',
			title: '공허의 딸',
			image: { full: 'Kaisa.png' },
			tags: ['Marksman']
		}
	}
};

describe('Data Dragon champion helpers', () => {
	it('normalizes champion ids into stable id slugs', () => {
		expect(normalizeChampionSlug('Lee Sin')).toBe('leesin');
		expect(normalizeChampionSlug("Kai'Sa")).toBe('kaisa');
	});

	it('creates id, key, name, and idSlug lookup maps', () => {
		const catalog = createChampionCatalog(championResponse);

		expect(findChampion(catalog, 'LeeSin')?.name).toBe('리 신');
		expect(findChampion(catalog, '64')?.id).toBe('LeeSin');
		expect(findChampion(catalog, '리 신')?.id).toBe('LeeSin');
		expect(findChampion(catalog, 'leesin')?.key).toBe('64');
	});

	it('generates same-origin proxy URLs', () => {
		const catalog = createChampionCatalog(championResponse);
		const champion = findChampion(catalog, 'Kaisa');

		expect(champion).toBeDefined();
		expect(championCatalogUrl('14.24.1')).toBe('/dd/cdn/14.24.1/data/ko_KR/champion.json');
		expect(championIconUrl(champion!, '14.24.1')).toBe('/dd/cdn/14.24.1/img/champion/Kaisa.png');
	});

	it('searches by champion name, id slug, key, and tags', () => {
		const catalog = createChampionCatalog(championResponse);

		expect(searchChampions(catalog, '리신').map((champion) => champion.id)).toEqual(['LeeSin']);
		expect(searchChampions(catalog, '리 신').map((champion) => champion.id)).toEqual(['LeeSin']);
		expect(searchChampions(catalog, 'kaisa').map((champion) => champion.id)).toEqual(['Kaisa']);
		expect(searchChampions(catalog, '145').map((champion) => champion.id)).toEqual(['Kaisa']);
		expect(searchChampions(catalog, 'fighter').map((champion) => champion.id)).toEqual(['LeeSin']);
	});

	it('ships every champion in the static fallback catalog', () => {
		const catalog = createChampionCatalog(fullChampionFixture);

		expect(catalog.champions).toHaveLength(173);
		expect(searchChampions(catalog, '', { limit: catalog.champions.length })).toHaveLength(173);
		expect(searchChampions(catalog, '스몰더').map((champion) => champion.id)).toEqual(['Smolder']);
	});

	it('falls back to a static fixture path when Data Dragon fetch fails', async () => {
		const fetcher = vi
			.fn()
			.mockResolvedValueOnce(new Response('upstream down', { status: 503 }))
			.mockResolvedValueOnce(Response.json(championResponse));

		const catalog = await loadChampionCatalog(fetcher, {
			fixtureUrl: '/fixtures/datadragon/champion-ko_KR.fixture.json'
		});

		expect(fetcher).toHaveBeenNthCalledWith(1, '/dd/api/versions.json');
		expect(fetcher).toHaveBeenNthCalledWith(2, '/fixtures/datadragon/champion-ko_KR.fixture.json');
		expect(catalog.version).toBe('14.24.1');
	});

	it('deduplicates concurrent catalog loads for the same public resource', async () => {
		const fetcher = vi.fn(async () => Response.json(championResponse));
		const options = {
			version: '14.24.1',
			proxyRoot: '/dd/cache-deduplication'
		};

		const [first, second] = await Promise.all([
			loadCachedChampionCatalog(fetcher, options),
			loadCachedChampionCatalog(fetcher, options)
		]);

		expect(fetcher).toHaveBeenCalledTimes(1);
		expect(first).toBe(second);
	});

	it('evicts a failed catalog load so a later request can recover', async () => {
		const fetcher = vi
			.fn()
			.mockRejectedValueOnce(new Error('temporary transport failure'))
			.mockResolvedValueOnce(Response.json(championResponse));
		const options = {
			version: '14.24.1',
			proxyRoot: '/dd/cache-recovery'
		};

		await expect(loadCachedChampionCatalog(fetcher, options)).rejects.toThrow(
			'temporary transport failure'
		);
		await expect(loadCachedChampionCatalog(fetcher, options)).resolves.toMatchObject({
			version: '14.24.1'
		});
		expect(fetcher).toHaveBeenCalledTimes(2);
	});
});
