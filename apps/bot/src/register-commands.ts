import { REST, Routes } from 'discord.js';
import { commands } from './commands/index.js';
import { assertCommandRegistrationScope, loadBotConfig } from './config.js';
import { verifyRestApplicationIdentity } from './discord/application-identity.js';
import { createLogger } from './log.js';

type RegistrationMode = 'guild' | 'global';

async function main(): Promise<void> {
	const mode = registrationMode(process.argv[2]);
	const config = loadBotConfig();
	assertCommandRegistrationScope(config, mode);
	const logger = createLogger(config);
	const rest = new REST({ version: '10' }).setToken(config.discordToken);
	await verifyRestApplicationIdentity(rest, config.applicationId);
	const body = commands.map((command) => command.data.toJSON());

	const route =
		mode === 'guild'
			? Routes.applicationGuildCommands(config.applicationId, requiredDevGuildId(config.devGuildId))
			: Routes.applicationCommands(config.applicationId);
	const registered = (await rest.put(route, { body })) as unknown[];
	logger.info(
		{
			mode,
			guildId: mode === 'guild' ? config.devGuildId : undefined,
			commandCount: registered.length
		},
		'Discord application commands registered'
	);
}

function registrationMode(value: string | undefined): RegistrationMode {
	if (value === 'guild' || value === 'global') return value;
	throw new Error('registration mode must be exactly "guild" or "global"');
}

function requiredDevGuildId(value: string | undefined): string {
	if (!value) {
		throw new Error('MONKEYBOT_BOT_DEV_GUILD_ID is required for guild command registration');
	}
	return value;
}

main().catch((error: unknown) => {
	process.stderr.write(
		`Discord command registration failed: ${error instanceof Error ? error.message : String(error)}\n`
	);
	process.exitCode = 1;
});
