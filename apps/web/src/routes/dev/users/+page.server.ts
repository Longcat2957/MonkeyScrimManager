import {
	dummyGuildCatalogDataSchema,
	dummyGroupsListDataSchema,
	dummyUsersListDataSchema
} from '$lib/contracts/dummyUsers';
import { meDataSchema, meGuildsDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, url }) => {
	const [userSnapshot, catalog, groupSnapshot, user, guildList] = await Promise.all([
		apiGet(
			fetch,
			apiRoute(['dev', 'dummy-users'], { limit: 100, offset: 0 }),
			dummyUsersListDataSchema,
			{
				url
			}
		),
		apiGet(fetch, apiRoute(['dev', 'dummy-users', 'catalog']), dummyGuildCatalogDataSchema, {
			url
		}),
		apiGet(fetch, apiRoute(['dev', 'dummy-groups']), dummyGroupsListDataSchema, { url }),
		apiGet(fetch, apiRoute(['me']), meDataSchema, { url }),
		apiGet(fetch, apiRoute(['me', 'guilds']), meGuildsDataSchema, { url })
	]);

	return {
		userSnapshot,
		dummyGuildCatalog: catalog.guilds,
		initialGuildId: catalog.guilds.some(
			(guild) => guild.guildId === url.searchParams.get('guildId')
		)
			? url.searchParams.get('guildId')
			: null,
		groups: groupSnapshot.groups,
		user,
		guilds: guildList.guilds
	};
};
