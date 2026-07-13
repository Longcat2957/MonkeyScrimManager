import { describe, expect, it } from 'vitest';
import { dummyGroupSchema, dummyUserSchema } from './dummyUsers';
import { dummyGroupFixture, dummyUserFixture } from './dummyUsers.testFixtures';

describe('dummy user contracts', () => {
	it('accepts an opaque dummy identity with real guild-domain state', () => {
		expect(dummyUserSchema.parse(dummyUserFixture)).toEqual(dummyUserFixture);
	});

	it('does not accept Discord identities or malformed nested state', () => {
		expect(
			dummyUserSchema.safeParse({ ...dummyUserFixture, userId: '1525847677542207548' }).success
		).toBe(false);
		expect(
			dummyUserSchema.safeParse({
				...dummyUserFixture,
				userId: dummyUserFixture.userId.toUpperCase()
			}).success
		).toBe(false);
		expect(
			dummyUserSchema.safeParse({
				...dummyUserFixture,
				guildMemberships: [{ ...dummyUserFixture.guildMemberships[0], mainLane: 'FLEX' }]
			}).success
		).toBe(false);
	});

	it('requires group member count to match the ordered member projection', () => {
		expect(dummyGroupSchema.parse(dummyGroupFixture)).toEqual(dummyGroupFixture);
		expect(dummyGroupSchema.safeParse({ ...dummyGroupFixture, memberCount: 2 }).success).toBe(
			false
		);
	});
});
