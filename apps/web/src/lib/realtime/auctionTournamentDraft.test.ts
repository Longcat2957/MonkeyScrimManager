import { describe, expect, it } from 'vitest';
import type { AuctionTournamentDraftSnapshot } from '$lib/contracts/auctionTournamentDraft';
import {
	auctionTournamentDraftRealtimeTopics,
	isNewerAuctionTournamentDraftInvalidation
} from './auctionTournamentDraft';

const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const topics = auctionTournamentDraftRealtimeTopics(guildId, tournamentId);
const snapshot = { tournament: { revision: 8 } } as AuctionTournamentDraftSnapshot;

describe('auction tournament draft realtime topic', () => {
	it('uses the canonical auction tournament topic', () => {
		expect(topics).toEqual({
			tournament: `guild:${guildId}:auction-tournament:${tournamentId}`,
			all: [`guild:${guildId}:auction-tournament:${tournamentId}`]
		});
	});

	it('only refreshes for a newer revision of the subscribed tournament', () => {
		const event = {
			type: 'invalidate' as const,
			eventId: '01900000-0000-7000-8000-000000000001',
			resourceId: tournamentId,
			occurredAt: '2026-07-15T04:00:00Z'
		};

		expect(
			isNewerAuctionTournamentDraftInvalidation(
				{ ...event, topic: topics.tournament, revision: 9 },
				snapshot,
				topics
			)
		).toBe(true);
		expect(
			isNewerAuctionTournamentDraftInvalidation(
				{ ...event, topic: topics.tournament, revision: 8 },
				snapshot,
				topics
			)
		).toBe(false);
		expect(
			isNewerAuctionTournamentDraftInvalidation(
				{ ...event, topic: `guild:${guildId}:auction-recruitment:${tournamentId}`, revision: 9 },
				snapshot,
				topics
			)
		).toBe(false);
	});
});
