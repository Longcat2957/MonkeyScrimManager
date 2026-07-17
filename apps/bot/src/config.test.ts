import { describe, expect, it } from 'vitest';

import {
	BotConfigError,
	assertCommandRegistrationScope,
	assertLegacyGlobalCleanupAllowed,
	loadBotConfig
} from './config.js';

const APPLICATION_ID = '900000000000000001';
const DEV_GUILD_ID = '800000000000000001';

function validEnvironment(overrides: NodeJS.ProcessEnv = {}): NodeJS.ProcessEnv {
	return {
		MONKEYBOT_ENV: 'development',
		MONKEYBOT_BOT_TOKEN: 'discord-bot-token',
		MONKEYBOT_DISCORD_APPLICATION_ID: APPLICATION_ID,
		MONKEYBOT_BOT_COMMAND_SCOPE: 'guild',
		MONKEYBOT_BOT_DEV_GUILD_ID: DEV_GUILD_ID,
		MONKEYBOT_BOT_API_BASE_URL: 'http://127.0.0.1:30020',
		MONKEYBOT_BOT_SERVICE_TOKEN: 'b'.repeat(32),
		MONKEYBOT_BOT_INSTANCE_ID: 'monkeybot-dev-1',
		MONKEYBOT_WEB_ORIGIN: 'http://localhost:5173',
		...overrides
	};
}

describe('Bot Discord environment contract', () => {
	it('uses one canonical application ID for development guild commands', () => {
		const config = loadBotConfig(validEnvironment());

		expect(config.applicationId).toBe(APPLICATION_ID);
		expect(config.commandScope).toBe('guild');
		expect(config.devGuildId).toBe(DEV_GUILD_ID);
	});

	it('accepts the legacy application ID only during migration', () => {
		const environment = validEnvironment({
			MONKEYBOT_DISCORD_APPLICATION_ID: '',
			MONKEYBOT_BOT_APPLICATION_ID: APPLICATION_ID
		});

		expect(loadBotConfig(environment).applicationId).toBe(APPLICATION_ID);
	});

	it('rejects canonical and legacy application ID mismatches', () => {
		expect(() =>
			loadBotConfig(validEnvironment({ MONKEYBOT_BOT_APPLICATION_ID: '900000000000000002' }))
		).toThrowError(
			new BotConfigError(
				'Invalid bot configuration: MONKEYBOT_DISCORD_APPLICATION_ID must match MONKEYBOT_BOT_APPLICATION_ID'
			)
		);
	});

	it('requires guild scope and an explicit guild outside production', () => {
		expect(() =>
			loadBotConfig(
				validEnvironment({
					MONKEYBOT_BOT_COMMAND_SCOPE: 'global',
					MONKEYBOT_BOT_DEV_GUILD_ID: ''
				})
			)
		).toThrow(/local\/development requires/);

		expect(() => loadBotConfig(validEnvironment({ MONKEYBOT_BOT_DEV_GUILD_ID: '' }))).toThrow(
			/requires MONKEYBOT_BOT_DEV_GUILD_ID/
		);
	});

	it('requires global scope without a development guild in production', () => {
		const production = loadBotConfig(
			validEnvironment({
				MONKEYBOT_ENV: 'production',
				MONKEYBOT_BOT_COMMAND_SCOPE: 'global',
				MONKEYBOT_BOT_DEV_GUILD_ID: '',
				MONKEYBOT_WEB_ORIGIN: 'https://monkeybot.lol'
			})
		);
		expect(production.commandScope).toBe('global');
		expect(production.devGuildId).toBeUndefined();

		expect(() =>
			loadBotConfig(
				validEnvironment({
					MONKEYBOT_ENV: 'production',
					MONKEYBOT_BOT_COMMAND_SCOPE: 'guild'
				})
			)
		).toThrow(/production requires/);
	});

	it('blocks registration outside the configured scope', () => {
		const development = loadBotConfig(validEnvironment());
		expect(() => assertCommandRegistrationScope(development, 'global')).toThrow(/forbidden/);
		expect(() => assertCommandRegistrationScope(development, 'guild')).not.toThrow();

		const production = loadBotConfig(
			validEnvironment({
				MONKEYBOT_ENV: 'production',
				MONKEYBOT_BOT_COMMAND_SCOPE: 'global',
				MONKEYBOT_BOT_DEV_GUILD_ID: '',
				MONKEYBOT_WEB_ORIGIN: 'https://monkeybot.lol'
			})
		);
		expect(() => assertCommandRegistrationScope(production, 'guild')).toThrow(/forbidden/);
	});

	it('allows legacy global cleanup only for the development application', () => {
		const development = loadBotConfig(validEnvironment());
		expect(() => assertLegacyGlobalCleanupAllowed(development)).not.toThrow();

		const production = loadBotConfig(
			validEnvironment({
				MONKEYBOT_ENV: 'production',
				MONKEYBOT_BOT_COMMAND_SCOPE: 'global',
				MONKEYBOT_BOT_DEV_GUILD_ID: '',
				MONKEYBOT_WEB_ORIGIN: 'https://monkeybot.lol'
			})
		);
		expect(() => assertLegacyGlobalCleanupAllowed(production)).toThrow(/only for/);
	});
});
