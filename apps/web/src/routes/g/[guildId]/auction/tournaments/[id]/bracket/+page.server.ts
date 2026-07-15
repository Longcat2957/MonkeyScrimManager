import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import { auctionTournamentBracketSnapshotSchema } from '$lib/contracts/auctionTournamentBracket';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-tournaments', params.id, 'bracket']),
		auctionTournamentBracketSnapshotSchema,
		{
			url,
			onApiError: (apiError) => {
				if (
					apiError.status === 409 &&
					apiError.code === 'auction_tournament_bracket_unavailable'
				) {
					redirect(
						303,
						resolve('/g/[guildId]/auction/tournaments/[id]/draft', {
							guildId: params.guildId,
							id: params.id
						})
					);
				}
			}
		}
	);

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
