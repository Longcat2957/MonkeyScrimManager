import { loadChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import { findMockAuctionRecruitment } from '$lib/fixtures/auctionWorkflow';
import { error } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch, params }) => {
	const recruitment = findMockAuctionRecruitment(params.id);
	if (!recruitment) error(404, '경매 모집을 찾을 수 없습니다.');

	return {
		guildId: params.guildId,
		recruitmentId: params.id,
		recruitment,
		championCatalog: await loadChampionCatalog(fetch, {
			fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
		})
	};
};
