import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import { auctionTournamentDraftSnapshotSchema } from '$lib/contracts/auctionTournamentDraft';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, url }) => {
	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-tournaments', params.id]),
		auctionTournamentDraftSnapshotSchema,
		{ url }
	);
	if (snapshot.tournament.status === 'BRACKET_SETUP') {
		redirect(
			303,
			resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
				guildId: params.guildId,
				id: params.id
			})
		);
	}
	if (
		snapshot.tournament.status === 'IN_GAME' ||
		snapshot.tournament.status === 'COMPLETED' ||
		snapshot.tournament.status === 'CANCELLED'
	) {
		redirect(
			303,
			resolve('/g/[guildId]/auction/tournaments/[id]/result', {
				guildId: params.guildId,
				id: params.id
			})
		);
	}

	return {
		guildId: params.guildId,
		snapshot
	};
};
