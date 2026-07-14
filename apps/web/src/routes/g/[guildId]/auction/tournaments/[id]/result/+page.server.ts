import { auctionTournamentResultSnapshotSchema } from '$lib/contracts/auctionTournamentResult';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-tournaments', params.id, 'result']),
		auctionTournamentResultSnapshotSchema,
		{ url }
	);

	return {
		guildId: params.guildId,
		snapshot
	};
};
