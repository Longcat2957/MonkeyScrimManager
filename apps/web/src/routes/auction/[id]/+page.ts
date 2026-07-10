import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch }) => {
	const championCatalog = await loadChampionCatalog(fetch, {
		fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
	});

	return {
		championCatalog
	};
};
