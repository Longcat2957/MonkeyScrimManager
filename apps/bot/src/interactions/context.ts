import type { MonkeyBotApiClient } from '../api/client.js';
import type { BotConfig } from '../config.js';
import type { Logger } from 'pino';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';

export interface InteractionContext {
	api: MonkeyBotApiClient;
	catalogSync: GuildCatalogSync;
	config: BotConfig;
	logger: Logger;
}
