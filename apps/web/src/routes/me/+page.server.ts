import { meDataSchema, meGuildsDataSchema, riotAccountsDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import { z } from 'zod';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, url }) => {
	const requestedTab = (url.searchParams.get('tab') ?? '').toLowerCase();
	const loadRiotAccounts = requestedTab === 'riot';

	const [user, guildList] = await Promise.all([
		apiGet(fetch, apiRoute(['me']), meDataSchema, { url }),
		apiGet(fetch, apiRoute(['me', 'guilds']), meGuildsDataSchema, { url })
	]);

	let riotAccounts: z.infer<typeof riotAccountsDataSchema>['riotAccounts'] = [];
	if (loadRiotAccounts) {
		try {
			const payload = await apiGet(fetch, apiRoute(['me', 'riot-accounts']), riotAccountsDataSchema, { url });
			riotAccounts = payload.riotAccounts;
		} catch {
			riotAccounts = [];
		}
	}
	return {
		user,
		guilds: guildList.guilds,
		defaultGuildId: user.defaultGuildId,
		riotAccounts
	};
};
