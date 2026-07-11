import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, params }) => {
	return {
		championCatalog: await loadChampionCatalog(fetch, {
			fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
		}),
		discordId: params.discordId,
		displayName: params.discordId === 'user-tamsol' ? '탐솔' : params.discordId
	};
};
