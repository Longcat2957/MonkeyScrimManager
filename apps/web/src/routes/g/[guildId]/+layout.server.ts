import { guildContextDataSchema, meDataSchema, meGuildsDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ fetch, params, url }) => {
	const [guild, user, guildList] = await Promise.all([
		apiGet(fetch, apiRoute(['guilds', params.guildId]), guildContextDataSchema, { url }),
		apiGet(fetch, apiRoute(['me']), meDataSchema, { url }),
		apiGet(fetch, apiRoute(['me', 'guilds']), meGuildsDataSchema, { url })
	]);

	return {
		guildId: params.guildId,
		guild,
		user,
		guilds: guildList.guilds
	};
};
