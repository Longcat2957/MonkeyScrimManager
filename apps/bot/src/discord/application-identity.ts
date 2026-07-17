import { Routes, type REST } from 'discord.js';

export class DiscordApplicationIdentityError extends Error {
	constructor(message: string) {
		super(message);
		this.name = 'DiscordApplicationIdentityError';
	}
}

export function assertDiscordApplicationIdentity(expectedId: string, actualId: unknown): void {
	if (typeof actualId !== 'string' || !/^[1-9][0-9]{0,19}$/.test(actualId)) {
		throw new DiscordApplicationIdentityError(
			'Discord did not return a valid application identity for the configured bot token'
		);
	}
	if (actualId !== expectedId) {
		throw new DiscordApplicationIdentityError(
			`Discord bot token application mismatch: expected ${expectedId}, received ${actualId}`
		);
	}
}

export async function verifyRestApplicationIdentity(
	rest: Pick<REST, 'get'>,
	expectedId: string
): Promise<void> {
	const application = await rest.get(Routes.oauth2CurrentApplication());
	const actualId =
		typeof application === 'object' && application !== null && 'id' in application
			? application.id
			: null;
	assertDiscordApplicationIdentity(expectedId, actualId);
}
