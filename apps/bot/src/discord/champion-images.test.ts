import { describe, expect, it, vi } from 'vitest';

import {
	championImageHref,
	normalizePatchVersion,
	resolveChampionImages
} from './champion-images.js';

const tinyPng = Buffer.from('89504e470d0a1a0a', 'hex');

describe('champion result image resolver', () => {
	it('normalizes short patches and only constructs canonical Data Dragon paths', () => {
		expect(normalizePatchVersion('15.14')).toBe('15.14.1');
		expect(normalizePatchVersion('15.14.2')).toBe('15.14.2');
		expect(normalizePatchVersion('latest')).toBeNull();
		expect(championImageHref('Ahri', '15.14')).toBe(
			'https://ddragon.leagueoflegends.com/cdn/15.14.1/img/champion/Ahri.png'
		);
		expect(championImageHref('Ahri<script>', '15.14')).toBeNull();
		expect(championImageHref('Ahri', null)).toBeNull();
	});

	it('deduplicates image fetches and resolves every valid external icon', async () => {
		const href = championImageHref('Ahri', '15.14')!;
		const resolveImage = vi.fn();
		const fetcher = vi.fn(
			async () =>
				new Response(tinyPng, {
					status: 200,
					headers: { 'content-type': 'image/png', 'content-length': String(tinyPng.length) }
				})
		);

		await resolveChampionImages(
			{ imagesToResolve: () => [href, href], resolveImage },
			{ fetcher: fetcher as typeof fetch }
		);

		expect(fetcher).toHaveBeenCalledOnce();
		expect(resolveImage).toHaveBeenCalledOnce();
		expect(resolveImage).toHaveBeenCalledWith(href, tinyPng);
	});

	it('leaves failed artwork unresolved without throwing', async () => {
		const href = championImageHref('Garen', '15.14')!;
		const resolveImage = vi.fn();

		await expect(
			resolveChampionImages(
				{ imagesToResolve: () => [href], resolveImage },
				{ fetcher: async () => new Response(null, { status: 404 }) }
			)
		).resolves.toBeUndefined();

		expect(resolveImage).not.toHaveBeenCalled();
	});
});
