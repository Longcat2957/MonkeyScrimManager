import pino, { type Logger } from 'pino';

import type { BotConfig } from './config.js';

const secretPaths = [
	'discordToken',
	'serviceToken',
	'config.discordToken',
	'config.serviceToken',
	'env.MONKEYBOT_BOT_TOKEN',
	'env.MONKEYBOT_BOT_SERVICE_TOKEN',
	'authorization',
	'headers.authorization',
	'req.headers.authorization',
	'request.headers.authorization',
	'res.headers["set-cookie"]',
	'cookies',
	'cookie'
];

export function createLogger(
	config: Pick<BotConfig, 'environment' | 'instanceId' | 'logLevel'>
): Logger {
	return pino({
		level: config.logLevel,
		base: {
			service: 'monkeybot-bot',
			environment: config.environment,
			instanceId: config.instanceId
		},
		redact: {
			paths: secretPaths,
			censor: '[REDACTED]'
		},
		serializers: {
			err: pino.stdSerializers.err
		}
	});
}
