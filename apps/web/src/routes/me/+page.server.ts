import {
	championPreferencesDataSchema,
	meDataSchema,
	meGuildsDataSchema,
	privacySettingsDataSchema,
	riotAccountsDataSchema,
	type ChampionPreferences,
	type PrivacySettings,
	type RiotAccount
} from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

const emptyChampionPreferences: ChampionPreferences = {
	TOP: [],
	JUNGLE: [],
	MID: [],
	BOTTOM: [],
	SUPPORT: []
};

const defaultPrivacySettings: PrivacySettings = {
	publicProfile: true,
	showRiotIds: true,
	allowExternalRecordSearch: false
};

export const load: PageServerLoad = async ({ fetch, url }) => {
	const requestedTab = (url.searchParams.get('tab') ?? '').toLowerCase();
	const initialTab =
		requestedTab === 'riot' || requestedTab === 'guild' || requestedTab === 'privacy'
			? requestedTab
			: 'champions';

	const [user, guildList] = await Promise.all([
		apiGet(fetch, apiRoute(['me']), meDataSchema, { url }),
		apiGet(fetch, apiRoute(['me', 'guilds']), meGuildsDataSchema, { url })
	]);

	let riotAccounts: RiotAccount[] = [];
	let championPreferences = emptyChampionPreferences;
	let privacySettings = defaultPrivacySettings;
	if (initialTab === 'riot') {
		const payload = await apiGet(fetch, apiRoute(['me', 'riot-accounts']), riotAccountsDataSchema, {
			url
		});
		riotAccounts = payload.riotAccounts;
	} else if (initialTab === 'champions') {
		const payload = await apiGet(
			fetch,
			apiRoute(['me', 'champion-preferences']),
			championPreferencesDataSchema,
			{ url }
		);
		championPreferences = payload.preferences;
	} else if (initialTab === 'privacy') {
		const payload = await apiGet(fetch, apiRoute(['me', 'privacy']), privacySettingsDataSchema, {
			url
		});
		privacySettings = payload.settings;
	}
	return {
		user,
		guilds: guildList.guilds,
		defaultGuildId: user.defaultGuildId,
		riotAccounts,
		championPreferences,
		privacySettings
	};
};
