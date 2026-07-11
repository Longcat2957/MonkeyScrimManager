import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, params, url }) => {
	const requestedView = url.searchParams.get('view');
	const initialView: 'setup' | 'lineup' | 'result' =
		requestedView === 'lineup' || requestedView === 'result' ? requestedView : 'setup';

	return {
		guildId: params.guildId,
		tournamentId: params.id,
		initialView,
		championCatalog: await loadChampionCatalog(fetch, {
			fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
		})
	};
};
