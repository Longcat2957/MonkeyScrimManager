import { guildAdminSeasonsDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, url }) => {
	const seasonSnapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'seasons']),
		guildAdminSeasonsDataSchema,
		{ url }
	);

	return { guildId: params.guildId, seasonSnapshot };
};
