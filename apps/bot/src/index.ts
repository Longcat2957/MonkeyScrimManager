import { Client, Events, GatewayIntentBits } from 'discord.js';
import { MonkeyBotApiClient } from './api/client.js';
import { loadBotConfig } from './config.js';
import { installGuildCatalogSync } from './discord/catalog-sync.js';
import { startHealthServer, type ReadinessState } from './health/server.js';
import { installInteractionDispatcher } from './interactions/dispatcher.js';
import { createLogger } from './log.js';

async function main(): Promise<void> {
	const config = loadBotConfig();
	const logger = createLogger(config);
	const api = new MonkeyBotApiClient({
		baseUrl: config.apiBaseUrl,
		serviceToken: config.serviceToken,
		timeoutMs: config.apiTimeoutMs,
		logger
	});
	const readiness: ReadinessState = {
		discord: { ready: false, checkedAt: null },
		api: { ready: false, checkedAt: null },
		shuttingDown: false
	};
	const health = await startHealthServer({
		host: config.healthHost,
		port: config.healthPort,
		getReadiness: () => readiness,
		logger
	});
	const refreshApiReadiness = async (): Promise<void> => {
		try {
			const report = await api.checkReadiness();
			readiness.api = {
				ready: Boolean(report?.acceptingRequests),
				checkedAt: new Date().toISOString()
			};
		} catch (error) {
			readiness.api = { ready: false, checkedAt: new Date().toISOString() };
			logger.warn({ err: error }, 'Bot API readiness check failed');
		}
	};
	void refreshApiReadiness();
	const readinessInterval = setInterval(
		() => void refreshApiReadiness(),
		config.heartbeatIntervalMs
	);
	readinessInterval.unref();
	const client = new Client({ intents: [GatewayIntentBits.Guilds] });
	const catalogSync = installGuildCatalogSync(client, api, logger);
	installInteractionDispatcher(client, { api, catalogSync, config, logger });

	client.once(Events.ClientReady, (readyClient) => {
		readiness.discord = { ready: true, checkedAt: new Date().toISOString() };
		logger.info(
			{ userId: readyClient.user.id, guildCount: readyClient.guilds.cache.size },
			'Discord bot ready'
		);
	});
	client.on(Events.Error, (error) => {
		readiness.discord = { ready: false, checkedAt: new Date().toISOString() };
		logger.error({ err: error }, 'Discord client error');
	});

	let shuttingDown = false;
	const shutdown = async (signal: NodeJS.Signals): Promise<void> => {
		if (shuttingDown) return;
		shuttingDown = true;
		readiness.shuttingDown = true;
		readiness.discord = { ready: false, checkedAt: new Date().toISOString() };
		clearInterval(readinessInterval);
		catalogSync.close();
		logger.info({ signal }, 'Discord bot shutting down');
		client.destroy();
		await health.close();
	};
	process.once('SIGINT', () => void shutdown('SIGINT'));
	process.once('SIGTERM', () => void shutdown('SIGTERM'));

	try {
		await client.login(config.discordToken);
	} catch (error) {
		logger.fatal({ err: error }, 'Discord login failed');
		await shutdown('SIGTERM');
		throw error;
	}
}

main().catch(() => {
	process.exitCode = 1;
});
