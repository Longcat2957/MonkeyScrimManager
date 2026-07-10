import { loadChampionCatalog, searchChampions, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, url }) => {
	const championCatalog = await loadChampionCatalog(fetch, {
		fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
	});
	const query = url.searchParams.get('q') ?? '';

	return {
		championCatalog,
		query,
		results: searchChampions(championCatalog, query, { limit: 40 })
	};
};
