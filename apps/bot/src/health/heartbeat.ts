import { Events, type Client } from 'discord.js';
import type { Logger } from 'pino';

import type { MonkeyBotApiClient } from '../api/client.js';
import type { HeartbeatRequest } from '../api/contracts.js';

type HeartbeatApi = Pick<MonkeyBotApiClient, 'heartbeat'>;

export interface BotHeartbeatReporter {
	report(): Promise<void>;
	close(): void;
}

export interface BotHeartbeatReporterOptions {
	instanceId: string;
	version: string;
	commitSha: string | null;
	intervalMs: number;
}

export function installBotHeartbeatReporter(
	client: Client,
	api: HeartbeatApi,
	logger: Logger,
	options: BotHeartbeatReporterOptions
): BotHeartbeatReporter {
	return new BotHeartbeatReporterManager(client, api, logger, options);
}

class BotHeartbeatReporterManager implements BotHeartbeatReporter {
	readonly #client: Client;
	readonly #api: HeartbeatApi;
	readonly #logger: Logger;
	readonly #request: HeartbeatRequest;
	readonly #intervalMs: number;
	readonly #abortController = new AbortController();
	#timer: NodeJS.Timeout | null = null;
	#activeReport: Promise<void> | null = null;
	#closed = false;

	readonly #onClientReady = () => {
		if (this.#closed || this.#timer) return;
		void this.report();
		this.#timer = setInterval(() => void this.report(), this.#intervalMs);
		this.#timer.unref();
	};

	constructor(
		client: Client,
		api: HeartbeatApi,
		logger: Logger,
		options: BotHeartbeatReporterOptions
	) {
		validateOptions(options);
		this.#client = client;
		this.#api = api;
		this.#logger = logger;
		this.#request = {
			instanceId: options.instanceId,
			version: options.version,
			commitSha: options.commitSha
		};
		this.#intervalMs = options.intervalMs;
		client.on(Events.ClientReady, this.#onClientReady);
	}

	report(): Promise<void> {
		if (this.#closed) return Promise.resolve();
		if (this.#activeReport) return this.#activeReport;

		const operation = this.#reportOnce().finally(() => {
			if (this.#activeReport === operation) this.#activeReport = null;
		});
		this.#activeReport = operation;
		return operation;
	}

	close(): void {
		if (this.#closed) return;
		this.#closed = true;
		if (this.#timer) clearInterval(this.#timer);
		this.#timer = null;
		this.#abortController.abort(new Error('Bot heartbeat reporter is shutting down'));
		this.#client.off(Events.ClientReady, this.#onClientReady);
	}

	async #reportOnce(): Promise<void> {
		try {
			await this.#api.heartbeat(this.#request, { signal: this.#abortController.signal });
		} catch (error) {
			if (this.#closed && this.#abortController.signal.aborted) return;
			this.#logger.warn(
				{ err: error, instanceId: this.#request.instanceId },
				'Bot heartbeat report failed'
			);
		}
	}
}

function validateOptions(options: BotHeartbeatReporterOptions): void {
	if (!/^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(options.instanceId)) {
		throw new Error('heartbeat reporter instanceId is invalid');
	}
	if (!/^[\x21-\x7e]{1,100}$/.test(options.version)) {
		throw new Error('heartbeat reporter version is invalid');
	}
	if (options.commitSha !== null && !/^[A-Za-z0-9._-]{1,128}$/.test(options.commitSha)) {
		throw new Error('heartbeat reporter commitSha is invalid');
	}
	if (!Number.isInteger(options.intervalMs) || options.intervalMs < 1_000) {
		throw new Error('heartbeat reporter intervalMs must be an integer of at least 1000');
	}
}
