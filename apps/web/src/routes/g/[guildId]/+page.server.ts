import { operationsDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import { mapOperations } from '$lib/workflow/presentation';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();
	const data = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'operations']),
		operationsDataSchema,
		{ url }
	);

	return {
		guildId: params.guildId,
		operations: mapOperations(data.operations)
	};
};
