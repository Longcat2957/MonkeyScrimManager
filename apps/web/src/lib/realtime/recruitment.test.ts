import { describe, expect, it } from 'vitest';
import type { RecruitmentSnapshot } from '$lib/contracts/recruitmentSnapshot';
import { isNewerRecruitmentInvalidation, recruitmentRealtimeTopic } from './recruitment';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const topic = recruitmentRealtimeTopic(guildId, recruitmentId);
const snapshot = { recruitment: { revision: 8 } } as RecruitmentSnapshot;

describe('recruitment realtime topic', () => {
	it('uses the canonical recruitment topic', () => {
		expect(topic).toBe(`guild:${guildId}:recruitment:${recruitmentId}`);
	});

	it('only refreshes for a newer revision of the subscribed recruitment', () => {
		const event = {
			type: 'invalidate' as const,
			eventId: '01900000-0000-7000-8000-000000000001',
			resourceId: recruitmentId,
			occurredAt: '2026-07-14T12:00:00Z'
		};

		expect(isNewerRecruitmentInvalidation({ ...event, topic, revision: 9 }, snapshot, topic)).toBe(
			true
		);
		expect(isNewerRecruitmentInvalidation({ ...event, topic, revision: 8 }, snapshot, topic)).toBe(
			false
		);
		expect(
			isNewerRecruitmentInvalidation(
				{
					...event,
					topic: `guild:${guildId}:recruitment-entry-draft:${recruitmentId}`,
					revision: 9
				},
				snapshot,
				topic
			)
		).toBe(false);
	});
});
