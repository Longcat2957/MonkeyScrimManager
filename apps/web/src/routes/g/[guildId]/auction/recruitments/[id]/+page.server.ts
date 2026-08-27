import { auctionRecruitmentSnapshotSchema } from '$lib/contracts/auctionRecruitmentSnapshot';
import { apiGet, apiRoute } from '$lib/server/api';
import { redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, url }) => {
	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-recruitments', params.id]),
		auctionRecruitmentSnapshotSchema,
		{ url }
	);
	if (snapshot.recruitment.status === 'CONVERTED') {
		redirect(303, snapshot.recruitment.href);
	}

	return {
		guildId: params.guildId,
		snapshot
	};
};
