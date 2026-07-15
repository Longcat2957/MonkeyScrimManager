import 'dotenv/config';

import { z } from 'zod';

const snowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const optionalSnowflakeSchema = z.preprocess(
	(value) => (typeof value === 'string' && value.trim() === '' ? undefined : value),
	snowflakeSchema.optional()
);
const positiveIntegerFromEnv = (fallback: number, minimum: number, maximum: number) =>
	z.preprocess(
		(value) => (value === undefined || value === '' ? fallback : value),
		z.coerce.number().int().min(minimum).max(maximum)
	);
const optionalTrimmedString = (schema: z.ZodType<string>) =>
	z.preprocess(
		(value) => (typeof value === 'string' && value.trim() === '' ? undefined : value),
		schema.optional()
	);
const httpUrlSchema = z.url().transform((value, context) => {
	const url = new URL(value);
	if (url.protocol !== 'http:' && url.protocol !== 'https:') {
		context.addIssue({ code: 'custom', message: 'must use http or https' });
		return z.NEVER;
	}
	return url;
});

const environmentSchema = z
	.object({
		MONKEYBOT_ENV: z.enum(['local', 'development', 'production', 'test']),
		MONKEYBOT_BOT_TOKEN: z.string().min(1),
		MONKEYBOT_BOT_APPLICATION_ID: snowflakeSchema,
		MONKEYBOT_BOT_DEV_GUILD_ID: optionalSnowflakeSchema,
		MONKEYBOT_BOT_API_BASE_URL: httpUrlSchema,
		MONKEYBOT_BOT_SERVICE_TOKEN: z.string().min(32),
		MONKEYBOT_BOT_INSTANCE_ID: z
			.string()
			.min(1)
			.max(128)
			.regex(/^[A-Za-z0-9][A-Za-z0-9._:-]*$/),
		MONKEYBOT_BOT_VERSION: z.preprocess(
			(value) => (value === undefined || value === '' ? '0.0.0' : value),
			z
				.string()
				.min(1)
				.max(100)
				.regex(/^[\x21-\x7e]+$/)
		),
		MONKEYBOT_BOT_COMMIT_SHA: optionalTrimmedString(
			z
				.string()
				.min(1)
				.max(128)
				.regex(/^[A-Za-z0-9._-]+$/)
		),
		MONKEYBOT_WEB_ORIGIN: httpUrlSchema,
		MONKEYBOT_BOT_HEALTH_ADDR: z.string().default('127.0.0.1:30030'),
		MONKEYBOT_BOT_API_TIMEOUT_MS: positiveIntegerFromEnv(10_000, 100, 120_000),
		MONKEYBOT_BOT_HEARTBEAT_INTERVAL_MS: positiveIntegerFromEnv(30_000, 1_000, 300_000),
		MONKEYBOT_BOT_NOTIFICATION_POLL_INTERVAL_MS: positiveIntegerFromEnv(1_000, 250, 30_000),
		MONKEYBOT_BOT_LOG_LEVEL: z
			.enum(['fatal', 'error', 'warn', 'info', 'debug', 'trace', 'silent'])
			.default('info')
	})
	.strip();

export type BotEnvironment = z.infer<typeof environmentSchema>['MONKEYBOT_ENV'];
export type BotLogLevel = z.infer<typeof environmentSchema>['MONKEYBOT_BOT_LOG_LEVEL'];

export interface BotConfig {
	environment: BotEnvironment;
	discordToken: string;
	applicationId: string;
	devGuildId?: string;
	apiBaseUrl: URL;
	serviceToken: string;
	instanceId: string;
	version: string;
	commitSha: string | null;
	webOrigin: URL;
	healthHost: string;
	healthPort: number;
	apiTimeoutMs: number;
	heartbeatIntervalMs: number;
	notificationPollIntervalMs: number;
	logLevel: BotLogLevel;
}

export class BotConfigError extends Error {
	constructor(message: string) {
		super(message);
		this.name = 'BotConfigError';
	}
}

export function loadBotConfig(environment: NodeJS.ProcessEnv = process.env): BotConfig {
	const parsed = environmentSchema.safeParse(environment);
	if (!parsed.success) {
		const fields = [
			...new Set(
				parsed.error.issues.map((issue) => issue.path.map(String).join('.') || 'environment')
			)
		];
		throw new BotConfigError(`Invalid bot configuration: ${fields.join(', ')}`);
	}

	const health = parseHealthAddress(parsed.data.MONKEYBOT_BOT_HEALTH_ADDR);
	const apiBaseUrl = normalizeBaseUrl(parsed.data.MONKEYBOT_BOT_API_BASE_URL);
	const webOrigin = normalizeOrigin(parsed.data.MONKEYBOT_WEB_ORIGIN, 'MONKEYBOT_WEB_ORIGIN');

	return {
		environment: parsed.data.MONKEYBOT_ENV,
		discordToken: parsed.data.MONKEYBOT_BOT_TOKEN,
		applicationId: parsed.data.MONKEYBOT_BOT_APPLICATION_ID,
		...(parsed.data.MONKEYBOT_BOT_DEV_GUILD_ID
			? { devGuildId: parsed.data.MONKEYBOT_BOT_DEV_GUILD_ID }
			: {}),
		apiBaseUrl,
		serviceToken: parsed.data.MONKEYBOT_BOT_SERVICE_TOKEN,
		instanceId: parsed.data.MONKEYBOT_BOT_INSTANCE_ID,
		version: parsed.data.MONKEYBOT_BOT_VERSION,
		commitSha: parsed.data.MONKEYBOT_BOT_COMMIT_SHA ?? null,
		webOrigin,
		healthHost: health.host,
		healthPort: health.port,
		apiTimeoutMs: parsed.data.MONKEYBOT_BOT_API_TIMEOUT_MS,
		heartbeatIntervalMs: parsed.data.MONKEYBOT_BOT_HEARTBEAT_INTERVAL_MS,
		notificationPollIntervalMs: parsed.data.MONKEYBOT_BOT_NOTIFICATION_POLL_INTERVAL_MS,
		logLevel: parsed.data.MONKEYBOT_BOT_LOG_LEVEL
	};
}

function normalizeBaseUrl(value: URL): URL {
	const url = new URL(value);
	if (url.username || url.password || url.search || url.hash) {
		throw new BotConfigError(
			'MONKEYBOT_BOT_API_BASE_URL must not contain credentials or query data'
		);
	}
	url.pathname = url.pathname.replace(/\/+$/, '') || '/';
	return url;
}

function normalizeOrigin(value: URL, field: string): URL {
	const url = new URL(value);
	if (
		url.username ||
		url.password ||
		url.search ||
		url.hash ||
		(url.pathname !== '/' && url.pathname)
	) {
		throw new BotConfigError(
			`${field} must be an origin without credentials, path, query, or hash`
		);
	}
	url.pathname = '/';
	return url;
}

function parseHealthAddress(value: string): { host: string; port: number } {
	let url: URL;
	try {
		url = new URL(`http://${value}`);
	} catch {
		throw new BotConfigError('MONKEYBOT_BOT_HEALTH_ADDR must be host:port');
	}
	const port = Number(url.port);
	if (
		!url.hostname ||
		!Number.isInteger(port) ||
		port < 1 ||
		port > 65_535 ||
		url.username ||
		url.password ||
		url.pathname !== '/' ||
		url.search ||
		url.hash
	) {
		throw new BotConfigError('MONKEYBOT_BOT_HEALTH_ADDR must be host:port');
	}
	return { host: url.hostname, port };
}
