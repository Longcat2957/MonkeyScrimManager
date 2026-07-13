import type { User } from 'discord.js';
import { describe, expect, it } from 'vitest';

import { actorFromInteraction } from './actor.js';

describe('actorFromInteraction', () => {
	it('keeps the global Discord name separate from the guild nickname', () => {
		const user = {
			id: '100000000000000001',
			username: 'longcat_login',
			globalName: 'Longcat',
			avatar: null
		} as User;

		expect(
			actorFromInteraction({
				user,
				member: { nick: 'MSM Longcat', roles: ['300000000000000001'] }
			})
		).toEqual({
			id: user.id,
			globalDisplayName: 'Longcat',
			displayName: 'MSM Longcat',
			avatarHash: null,
			roleIds: ['300000000000000001']
		});
	});

	it('falls back to the Discord username for both missing names', () => {
		const user = {
			id: '100000000000000001',
			username: 'longcat_login',
			globalName: null,
			avatar: 'avatar-hash'
		} as User;

		expect(actorFromInteraction({ user, member: { roles: [] } })).toMatchObject({
			globalDisplayName: 'longcat_login',
			displayName: 'longcat_login'
		});
	});
});
