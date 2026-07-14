import { recruitmentSnapshotSchema } from '$lib/contracts/recruitmentSnapshot';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'recruitments', params.id]),
		recruitmentSnapshotSchema,
		{ url }
	);

	return {
		guildId: params.guildId,
		snapshot
	};
};
