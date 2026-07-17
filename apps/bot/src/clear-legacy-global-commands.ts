import { REST, Routes } from 'discord.js';
import { assertLegacyGlobalCleanupAllowed, loadBotConfig } from './config.js';
import { verifyRestApplicationIdentity } from './discord/application-identity.js';
import { createLogger } from './log.js';

async function main(): Promise<void> {
	const config = loadBotConfig();
	assertLegacyGlobalCleanupAllowed(config);
	const logger = createLogger(config);
	const rest = new REST({ version: '10' }).setToken(config.discordToken);
	await verifyRestApplicationIdentity(rest, config.applicationId);
	await rest.put(Routes.applicationCommands(config.applicationId), { body: [] });
	logger.warn(
		{ applicationId: config.applicationId },
		'Legacy global Discord application commands cleared for development application'
	);
}

main().catch((error: unknown) => {
	process.stderr.write(
		`Discord legacy global command cleanup failed: ${error instanceof Error ? error.message : String(error)}\n`
	);
	process.exitCode = 1;
});
