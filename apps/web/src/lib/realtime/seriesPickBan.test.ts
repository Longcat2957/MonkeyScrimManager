import { describe, expect, it } from 'vitest';
import type { SeriesPickBanSnapshot } from '$lib/contracts/seriesPickBan';
import { isNewerSeriesPickBanInvalidation, seriesPickBanRealtimeTopics } from './seriesPickBan';

const guildId = '1525851181157580861';
const seriesId = '01900000-0000-7000-8000-000000000101';
const snapshot = { series: { revision: 7 } } as SeriesPickBanSnapshot;

describe('series pick-ban realtime topic', () => {
	it('uses the single series topic and only reloads newer series revisions', () => {
		const topics = seriesPickBanRealtimeTopics(guildId, seriesId);
		expect(topics).toEqual({
			series: `guild:${guildId}:series:${seriesId}`,
			all: [`guild:${guildId}:series:${seriesId}`]
		});

		const event = {
			type: 'invalidate' as const,
			eventId: '01900000-0000-7000-8000-000000000001',
			topic: topics.series,
			resourceId: seriesId,
			revision: 8,
			occurredAt: '2026-07-14T12:00:00Z'
		};
		expect(isNewerSeriesPickBanInvalidation(event, snapshot, topics)).toBe(true);
		expect(isNewerSeriesPickBanInvalidation({ ...event, revision: 7 }, snapshot, topics)).toBe(
			false
		);
		expect(
			isNewerSeriesPickBanInvalidation(
				{ ...event, topic: `guild:${guildId}:series:other` },
				snapshot,
				topics
			)
		).toBe(false);
	});
});
