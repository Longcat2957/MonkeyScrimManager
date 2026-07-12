import type { MonkeyBotApiClient } from '../api/client.js';
import type { BotConfig } from '../config.js';
import type { Logger } from 'pino';

export interface InteractionContext {
	api: MonkeyBotApiClient;
	config: BotConfig;
	logger: Logger;
}
