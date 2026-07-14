import { auctionRecruitmentSnapshotSchema } from '$lib/contracts/auctionRecruitmentSnapshot';
import { auctionSetupDraftSnapshotSchema } from '$lib/contracts/auctionTournamentSetup';
import { apiGet, apiRoute } from '$lib/server/api';
import { redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-recruitments', params.id]),
		auctionRecruitmentSnapshotSchema,
		{ url }
	);

	if (snapshot.recruitment.status === 'OPEN') {
		redirect(303, `/g/${params.guildId}/auction/recruitments/${params.id}`);
	}

	const setup = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-recruitments', params.id, 'setup-draft']),
		auctionSetupDraftSnapshotSchema,
		{ url }
	);

	return {
		guildId: params.guildId,
		snapshot,
		setup
	};
};
