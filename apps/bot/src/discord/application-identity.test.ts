import { describe, expect, it, vi } from 'vitest';

import {
	DiscordApplicationIdentityError,
	assertDiscordApplicationIdentity,
	verifyRestApplicationIdentity
} from './application-identity.js';

const APPLICATION_ID = '900000000000000001';

describe('Discord application identity guard', () => {
	it('accepts the expected application returned by Discord', async () => {
		const rest = { get: vi.fn(async () => ({ id: APPLICATION_ID, name: 'MonkeyBot[dev]' })) };

		await expect(verifyRestApplicationIdentity(rest, APPLICATION_ID)).resolves.toBeUndefined();
		expect(rest.get).toHaveBeenCalledOnce();
	});

	it('rejects a bot token belonging to another application', () => {
		expect(() =>
			assertDiscordApplicationIdentity(APPLICATION_ID, '900000000000000002')
		).toThrowError(DiscordApplicationIdentityError);
	});

	it('rejects malformed Discord identity responses without including the response body', async () => {
		const rest = { get: vi.fn(async () => ({ token: 'must-not-appear' })) };

		await expect(verifyRestApplicationIdentity(rest, APPLICATION_ID)).rejects.toThrow(
			'Discord did not return a valid application identity'
		);
	});
});
