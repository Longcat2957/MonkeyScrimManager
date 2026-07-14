import { describe, expect, it } from 'vitest';
import type { AuctionRecruitmentSnapshot } from '$lib/contracts/auctionRecruitmentSnapshot';
import {
	isNewerAuctionRecruitmentInvalidation,
	auctionRecruitmentRealtimeTopic
} from './auctionRecruitment';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const topic = auctionRecruitmentRealtimeTopic(guildId, recruitmentId);
const snapshot = { recruitment: { revision: 8 } } as AuctionRecruitmentSnapshot;

describe('auction recruitment realtime topic', () => {
	it('uses the canonical auction recruitment topic', () => {
		expect(topic).toBe(`guild:${guildId}:auction-recruitment:${recruitmentId}`);
	});

	it('only refreshes for a newer revision of the subscribed recruitment', () => {
		const event = {
			type: 'invalidate' as const,
			eventId: '01900000-0000-7000-8000-000000000001',
			resourceId: recruitmentId,
			occurredAt: '2026-07-14T12:00:00Z'
		};

		expect(
			isNewerAuctionRecruitmentInvalidation({ ...event, topic, revision: 9 }, snapshot, topic)
		).toBe(true);
		expect(
			isNewerAuctionRecruitmentInvalidation({ ...event, topic, revision: 8 }, snapshot, topic)
		).toBe(false);
		expect(
			isNewerAuctionRecruitmentInvalidation(
				{ ...event, topic: `guild:${guildId}:recruitment:${recruitmentId}`, revision: 9 },
				snapshot,
				topic
			)
		).toBe(false);
	});
});
