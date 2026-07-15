import {
	auctionRecruitmentListDataSchema,
	auctionTournamentListDataSchema
} from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const [recruitmentData, activeTournamentData, completedTournamentData] = await Promise.all([
		apiGet(
			fetch,
			apiRoute(['guilds', params.guildId, 'auction-recruitments'], {
				state: 'actionable',
				limit: 100
			}),
			auctionRecruitmentListDataSchema,
			{ url }
		),
		apiGet(
			fetch,
			apiRoute(['guilds', params.guildId, 'auction-tournaments'], {
				state: 'active',
				limit: 100
			}),
			auctionTournamentListDataSchema,
			{ url }
		),
		apiGet(
			fetch,
			apiRoute(['guilds', params.guildId, 'auction-tournaments'], {
				state: 'completed',
				limit: 100
			}),
			auctionTournamentListDataSchema,
			{ url }
		)
	]);

	return {
		guildId: params.guildId,
		auctionPages: {
			actionableRecruitments: recruitmentData,
			activeTournaments: activeTournamentData,
			completedTournaments: completedTournamentData
		}
	};
};
