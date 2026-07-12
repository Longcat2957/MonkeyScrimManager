import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import { findMockAuctionTournament } from '$lib/fixtures/auctionWorkflow';
import { error } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, params, url }) => {
	const requestedView = url.searchParams.get('view');
	const initialView: 'setup' | 'lineup' | 'result' =
		requestedView === 'lineup' || requestedView === 'result' ? requestedView : 'setup';
	const tournament = findMockAuctionTournament(params.id);
	if (!tournament) error(404, '경매 토너먼트를 찾을 수 없습니다.');

	return {
		guildId: params.guildId,
		tournamentId: params.id,
		tournament,
		initialView,
		championCatalog: await loadChampionCatalog(fetch, {
			fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
		})
	};
};
