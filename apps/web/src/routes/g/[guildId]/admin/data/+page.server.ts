import { guildAdminMembersDataSchema, guildAdminSeasonsDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, url }) => {
	const [memberSnapshot, seasonSnapshot] = await Promise.all([
		apiGet(fetch, apiRoute(['guilds', params.guildId, 'members']), guildAdminMembersDataSchema, {
			url
		}),
		apiGet(fetch, apiRoute(['guilds', params.guildId, 'seasons']), guildAdminSeasonsDataSchema, {
			url
		})
	]);

	return { guildId: params.guildId, memberSnapshot, seasonSnapshot };
};
