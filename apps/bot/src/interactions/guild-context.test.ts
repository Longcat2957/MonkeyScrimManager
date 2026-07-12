import { describe, expect, it } from 'vitest';
import {
	BOT_NOT_INSTALLED_MESSAGE,
	SERVER_ONLY_MESSAGE,
	guildInteractionError
} from './guild-context.js';

describe('guildInteractionError', () => {
	it('rejects direct-message interactions', () => {
		expect(guildInteractionError({ guildId: null, guild: null, member: null })).toBe(
			SERVER_ONLY_MESSAGE
		);
	});

	it('distinguishes command-only integrations from an installed bot', () => {
		expect(guildInteractionError({ guildId: '1', guild: null, member: null })).toBe(
			BOT_NOT_INSTALLED_MESSAGE
		);
	});

	it('accepts interactions from a cached guild membership', () => {
		expect(guildInteractionError({ guildId: '1', guild: {}, member: {} })).toBeNull();
	});
});
