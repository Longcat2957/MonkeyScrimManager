import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ data, fetch }) => ({
	...data,
	championCatalog: await loadChampionCatalog(fetch, {
		fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
	})
});
