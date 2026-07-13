import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, url, data }) => {
	const requestedTab = (url.searchParams.get('tab') ?? '').toLowerCase();
	const initialTab: 'champions' | 'riot' | 'guild' | 'privacy' =
		requestedTab === 'riot' || requestedTab === 'guild' || requestedTab === 'privacy'
			? requestedTab
			: 'champions';

	return {
		initialTab,
		defaultGuildId: data.defaultGuildId,
		user: data.user,
		guilds: data.guilds,
		riotAccounts: data.riotAccounts,
		championPreferences: data.championPreferences,
		privacySettings: data.privacySettings,
		championCatalog:
			initialTab === 'champions'
				? await loadChampionCatalog(fetch, {
						fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
					})
				: null
	};
};
