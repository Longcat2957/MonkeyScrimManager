import { describe, expect, it } from 'vitest';
import type { EntryDraftSnapshot } from '$lib/contracts/entryDraft';
import { entryDraftRealtimeTopics, isNewerEntryDraftInvalidation } from './entryDraft';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const snapshot = {
	recruitment: { revision: 8 },
	draft: { revision: 12 }
} as EntryDraftSnapshot;
const event = {
	type: 'invalidate' as const,
	eventId: '01900000-0000-7000-8000-000000000001',
	resourceId: recruitmentId,
	occurredAt: '2026-07-14T12:00:00Z'
};

describe('entry draft realtime topics', () => {
	it('uses separate canonical topics for the recruitment and draft revision domains', () => {
		expect(entryDraftRealtimeTopics(guildId, recruitmentId)).toEqual({
			recruitment: `guild:${guildId}:recruitment:${recruitmentId}`,
			draft: `guild:${guildId}:recruitment-entry-draft:${recruitmentId}`,
			all: [
				`guild:${guildId}:recruitment:${recruitmentId}`,
				`guild:${guildId}:recruitment-entry-draft:${recruitmentId}`
			]
		});
	});

	it('compares each topic to only its own snapshot revision and ignores duplicate or reverse events', () => {
		const topics = entryDraftRealtimeTopics(guildId, recruitmentId);

		expect(
			isNewerEntryDraftInvalidation(
				{ ...event, topic: topics.recruitment, revision: 9 },
				snapshot,
				topics
			)
		).toBe(true);
		expect(
			isNewerEntryDraftInvalidation(
				{ ...event, topic: topics.recruitment, revision: 8 },
				snapshot,
				topics
			)
		).toBe(false);
		expect(
			isNewerEntryDraftInvalidation(
				{ ...event, topic: topics.draft, revision: 13 },
				snapshot,
				topics
			)
		).toBe(true);
		expect(
			isNewerEntryDraftInvalidation(
				{ ...event, topic: topics.draft, revision: 12 },
				snapshot,
				topics
			)
		).toBe(false);
	});
});
