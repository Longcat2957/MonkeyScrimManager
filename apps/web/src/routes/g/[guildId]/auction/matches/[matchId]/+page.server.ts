import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import { loadCachedChampionCatalog, STATIC_CHAMPION_FIXTURE_URL } from '$lib/datadragon';
import {
	auctionTournamentBracketSnapshotSchema,
	type AuctionTournamentBracketSnapshot
} from '$lib/contracts/auctionTournamentBracket';
import {
	auctionMatchGameResultSnapshotSchema,
	type AuctionMatchGameResultSnapshot
} from '$lib/contracts/auctionMatchGameResult';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

function requiresTournamentLineupStage(
	snapshot: AuctionMatchGameResultSnapshot,
	bracket: AuctionTournamentBracketSnapshot
): boolean {
	if (
		bracket.tournament.status === 'COMPLETED' ||
		bracket.tournament.status === 'CANCELLED' ||
		bracket.tournament.format !== 'TWENTY_PERSON' ||
		!bracket.bracket.locked ||
		(snapshot.match.matchKey !== 'SEMIFINAL_A' && snapshot.match.matchKey !== 'SEMIFINAL_B')
	) {
		return false;
	}

	const semifinals = bracket.bracket.matches.filter(
		(match) => match.matchKey === 'SEMIFINAL_A' || match.matchKey === 'SEMIFINAL_B'
	);
	return semifinals.length !== 2 || !semifinals.every((match) => match.lineupsLocked);
}

export const load: PageServerLoad = async ({ fetch, params, url }) => {
	const snapshot = await apiGet(
		fetch,
		apiRoute(['guilds', params.guildId, 'auction-matches', params.matchId]),
		auctionMatchGameResultSnapshotSchema,
		{ url }
	);

	if (snapshot.match.matchKey === 'SEMIFINAL_A' || snapshot.match.matchKey === 'SEMIFINAL_B') {
		const bracket = await apiGet(
			fetch,
			apiRoute([
				'guilds',
				params.guildId,
				'auction-tournaments',
				snapshot.match.tournamentId,
				'bracket'
			]),
			auctionTournamentBracketSnapshotSchema,
			{ url }
		);
		if (requiresTournamentLineupStage(snapshot, bracket)) {
			redirect(
				303,
				resolve('/g/[guildId]/auction/tournaments/[id]/lineup', {
					guildId: params.guildId,
					id: snapshot.match.tournamentId
				})
			);
		}
	}

	const championCatalog = await loadCachedChampionCatalog(fetch, {
		fixtureUrl: STATIC_CHAMPION_FIXTURE_URL
	});

	return {
		guildId: params.guildId,
		snapshot,
		championCatalog
	};
};
