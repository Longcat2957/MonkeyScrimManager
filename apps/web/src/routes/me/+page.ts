import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, url }) => {
	const requestedTab = url.searchParams.get('tab');
	const initialTab: 'champions' | 'riot' | 'guild' | 'privacy' =
		requestedTab === 'riot' || requestedTab === 'guild' || requestedTab === 'privacy'
			? requestedTab
			: 'champions';

	return {
		initialTab,
		championCatalog: await loadChampionCatalog(fetch, {
			fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
		})
	};
};
