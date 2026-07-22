import {
	loadCachedChampionCatalog,
	STATIC_CHAMPION_FIXTURE_URL,
	type ChampionCatalog
} from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ data, fetch }) => {
	let championCatalog: ChampionCatalog | null = null;
	if (data.records) {
		try {
			championCatalog = await loadCachedChampionCatalog(fetch, {
				fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
			});
		} catch {
			championCatalog = null;
		}
	}

	return {
		query: data.query,
		lookup: data.lookup,
		records: data.records,
		loadError: data.loadError,
		championCatalog
	};
};
