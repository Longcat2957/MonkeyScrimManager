import type { AuctionTournamentDraftSnapshot } from '$lib/contracts/auctionTournamentDraft';
import type { RealtimeInvalidateEvent } from './browserSocket';

export interface AuctionTournamentDraftRealtimeTopics {
	tournament: string;
	all: readonly [string];
}

export function auctionTournamentDraftRealtimeTopics(
	guildId: string,
	tournamentId: string
): AuctionTournamentDraftRealtimeTopics {
	const tournament = `guild:${guildId}:auction-tournament:${tournamentId}`;
	return { tournament, all: [tournament] };
}

export function isNewerAuctionTournamentDraftInvalidation(
	event: RealtimeInvalidateEvent,
	snapshot: AuctionTournamentDraftSnapshot,
	topics: AuctionTournamentDraftRealtimeTopics
): boolean {
	return event.topic === topics.tournament && event.revision > snapshot.tournament.revision;
}
