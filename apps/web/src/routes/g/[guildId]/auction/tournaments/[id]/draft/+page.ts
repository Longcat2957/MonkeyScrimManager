import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import { findMockAuctionTournament } from '$lib/fixtures/auctionWorkflow';
import { error } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, params }) => {
	const tournament = findMockAuctionTournament(params.id);
	if (!tournament) error(404, '경매 토너먼트를 찾을 수 없습니다.');

	return {
		guildId: params.guildId,
		tournamentId: params.id,
		tournament,
		championCatalog: await loadChampionCatalog(fetch, {
			fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
		})
	};
};
