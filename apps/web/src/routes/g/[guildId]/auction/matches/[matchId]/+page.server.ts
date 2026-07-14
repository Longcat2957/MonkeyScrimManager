import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import { auctionMatchGameResultSnapshotSchema } from '$lib/contracts/auctionMatchGameResult';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const [snapshot, championCatalog] = await Promise.all([
		apiGet(
			fetch,
			apiRoute(['guilds', params.guildId, 'auction-matches', params.matchId]),
			auctionMatchGameResultSnapshotSchema,
			{ url }
		),
		loadChampionCatalog(fetch, { fixtureUrl: STATIC_CHAMPION_FIXTURE_URL })
	]);

	return {
		guildId: params.guildId,
		snapshot,
		championCatalog
	};
};
