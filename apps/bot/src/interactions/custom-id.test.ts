import { describe, expect, it } from 'vitest';

import { parseRecruitmentCustomId, recruitmentCustomId } from './custom-id.js';

const recruitmentId = '01900000-0000-7000-8000-000000000101';

describe('recruitment custom IDs', () => {
	it('roundtrips every supported component action', () => {
		for (const action of ['join', 'leave', 'roles', 'start-entry'] as const) {
			const customId = recruitmentCustomId(action, recruitmentId);
			expect(customId.length).toBeLessThanOrEqual(100);
			expect(parseRecruitmentCustomId(customId)).toEqual({ action, recruitmentId });
		}
	});

	it.each([
		'',
		`other:join:${recruitmentId}`,
		`recruit:unknown:${recruitmentId}`,
		`recruit:join:not-a-uuid`,
		`recruit:join:${recruitmentId}:extra`
	])('rejects malformed or foreign IDs: %s', (customId) => {
		expect(parseRecruitmentCustomId(customId)).toBeNull();
	});

	it('refuses to generate an ID for a non-UUID resource', () => {
		expect(() => recruitmentCustomId('join', '214')).toThrow('requires a UUID');
	});
});
