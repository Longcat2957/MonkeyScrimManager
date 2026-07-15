import { resolve } from '$app/paths';
import { error, redirect } from '@sveltejs/kit';
import {
	auctionTournamentBracketSnapshotSchema,
	type AuctionTournamentBracketMatch
} from '$lib/contracts/auctionTournamentBracket';
import {
	auctionMatchGameResultSnapshotSchema,
	type AuctionMatchGameResultSnapshot
} from '$lib/contracts/auctionMatchGameResult';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

function semifinalMatches(
	matches: AuctionTournamentBracketMatch[]
): [AuctionTournamentBracketMatch, AuctionTournamentBracketMatch] | null {
	const semifinalA = matches.find((match) => match.matchKey === 'SEMIFINAL_A');
	const semifinalB = matches.find((match) => match.matchKey === 'SEMIFINAL_B');
	if (
		!semifinalA ||
		!semifinalB ||
		semifinalA.leftTeamId === null ||
		semifinalA.rightTeamId === null ||
		semifinalB.leftTeamId === null ||
		semifinalB.rightTeamId === null
	) {
		return null;
	}
	return [semifinalA, semifinalB];
}

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const bracket = await apiGet(
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

	if (bracket.tournament.format !== 'TWENTY_PERSON' || !bracket.bracket.locked) {
		redirect(
			303,
			resolve('/g/[guildId]/auction/tournaments/[id]/bracket', {
				guildId: params.guildId,
				id: params.id
			})
		);
	}

	const matches = semifinalMatches(bracket.bracket.matches);
	if (!matches) {
		error(409, '준결승 대진이 확정된 뒤 20인 라인업을 편성할 수 있습니다.');
	}

	const semifinalSnapshots = await Promise.all(
		matches.map((match) =>
			apiGet(
				fetch,
				apiRoute(['guilds', params.guildId, 'auction-matches', match.id]),
				auctionMatchGameResultSnapshotSchema,
				{ url }
			)
		)
	);

	return {
		guildId: params.guildId,
		bracket,
		semifinalSnapshots: semifinalSnapshots as [
			AuctionMatchGameResultSnapshot,
			AuctionMatchGameResultSnapshot
		]
	};
};
