import { auctionRecruitmentSnapshotSchema } from '$lib/contracts/auctionRecruitmentSnapshot';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-recruitments', params.id]),
		auctionRecruitmentSnapshotSchema,
		{ url }
	);

	return {
		guildId: params.guildId,
		snapshot
	};
};
