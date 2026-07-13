import type { Logger } from 'pino';
import { type ZodType } from 'zod';

import {
	actorRequestSchema,
	apiReadinessDataSchema,
	apiErrorEnvelopeSchema,
	bindRecruitmentMessageRequestSchema,
	createRecruitmentRequestSchema,
	discordCatalogSyncDataSchema,
	discordCatalogSyncRequestSchema,
	joinRecruitmentRequestSchema,
	recruitmentSnapshotSchema,
	setPreferredLanesRequestSchema,
	setupGuildDataSchema,
	setupGuildRequestSchema,
	successEnvelopeSchema,
	type ActorRequest,
	type ApiReadinessData,
	type BindRecruitmentMessageRequest,
	type CreateRecruitmentRequest,
	type DiscordCatalogSyncData,
	type DiscordCatalogSyncRequest,
	type JoinRecruitmentRequest,
	type RecruitmentSnapshot,
	type SetPreferredLanesRequest,
	type SetupGuildData,
	type SetupGuildRequest
} from './contracts.js';

export interface ApiMutationContext {
	idempotencyKey: string;
	signal?: AbortSignal;
}

export interface ApiRequestContext {
	signal?: AbortSignal;
}

export interface MonkeyBotApiClientOptions {
	baseUrl: URL | string;
	serviceToken: string;
	timeoutMs: number;
	fetch?: typeof globalThis.fetch;
	logger?: Logger;
}

interface MutationRequest<TRequest, TResponse> {
	method: 'POST' | 'PUT' | 'DELETE';
	segments: string[];
	body: TRequest;
	requestSchema: ZodType<TRequest>;
	responseSchema: ZodType<TResponse>;
	context: ApiRequestContext;
	idempotencyKey: string | null;
	logRequest: boolean;
}

export class MonkeyBotApiError extends Error {
	readonly status: number;
	readonly code: string;
	readonly requestId: string | null;
	readonly details: unknown;

	constructor(options: {
		status: number;
		code: string;
		message: string;
		requestId?: string | null;
		details?: unknown;
	}) {
		super(options.message);
		this.name = 'MonkeyBotApiError';
		this.status = options.status;
		this.code = options.code;
		this.requestId = options.requestId ?? null;
		this.details = options.details;
	}
}

export class MonkeyBotApiContractError extends Error {
	constructor(message: string, options?: ErrorOptions) {
		super(message, options);
		this.name = 'MonkeyBotApiContractError';
	}
}

export class MonkeyBotApiTransportError extends Error {
	constructor(message: string, options?: ErrorOptions) {
		super(message, options);
		this.name = 'MonkeyBotApiTransportError';
	}
}

export class MonkeyBotApiClient {
	readonly #baseUrl: URL;
	readonly #serviceToken: string;
	readonly #timeoutMs: number;
	readonly #fetch: typeof globalThis.fetch;
	readonly #logger: Logger | undefined;

	constructor(options: MonkeyBotApiClientOptions) {
		this.#baseUrl = normalizeBaseUrl(options.baseUrl);
		if (options.serviceToken.length < 32) {
			throw new MonkeyBotApiContractError('service token must contain at least 32 characters');
		}
		if (!Number.isInteger(options.timeoutMs) || options.timeoutMs < 100) {
			throw new MonkeyBotApiContractError('timeoutMs must be an integer of at least 100');
		}
		this.#serviceToken = options.serviceToken;
		this.#timeoutMs = options.timeoutMs;
		this.#fetch = options.fetch ?? globalThis.fetch;
		this.#logger = options.logger;
	}

	async checkReadiness(signal?: AbortSignal): Promise<ApiReadinessData | null> {
		const url = this.#url(['readyz']);
		const controller = new AbortController();
		const timeout = setTimeout(
			() => controller.abort(new Error('API readiness request timed out')),
			this.#timeoutMs
		);
		timeout.unref();
		const abortFromCaller = () => controller.abort(signal?.reason);
		signal?.addEventListener('abort', abortFromCaller, { once: true });

		let response: Response;
		try {
			response = await this.#fetch(url, {
				method: 'GET',
				headers: {
					accept: 'application/json',
					authorization: `Bearer ${this.#serviceToken}`
				},
				redirect: 'error',
				signal: controller.signal
			});
		} catch (error) {
			const timedOut = controller.signal.aborted && !signal?.aborted;
			throw new MonkeyBotApiTransportError(
				timedOut ? 'Bot API readiness request timed out' : 'Bot API readiness request failed',
				{ cause: error }
			);
		} finally {
			clearTimeout(timeout);
			signal?.removeEventListener('abort', abortFromCaller);
		}

		const payload = await readJson(response);
		if (response.status === 503) {
			const error = apiErrorEnvelopeSchema.safeParse(payload);
			if (!error.success) {
				throw new MonkeyBotApiContractError(
					'Bot API readiness error does not match the expected contract'
				);
			}
			return null;
		}
		if (!response.ok) {
			const error = apiErrorEnvelopeSchema.safeParse(payload);
			throw new MonkeyBotApiError({
				status: response.status,
				code: error.success ? error.data.error.code : 'invalid_error_response',
				message: error.success
					? error.data.error.message
					: 'Bot API rejected the readiness request',
				requestId: error.success
					? error.data.error.requestId
					: response.headers.get('x-request-id'),
				details: error.success ? error.data.error.details : undefined
			});
		}

		const envelope = successEnvelopeSchema(apiReadinessDataSchema).safeParse(payload);
		if (!envelope.success) {
			throw new MonkeyBotApiContractError(
				'Bot API readiness response does not match the expected contract'
			);
		}
		return envelope.data.data;
	}

	setupGuild(
		guildId: string,
		input: SetupGuildRequest,
		context: ApiMutationContext
	): Promise<SetupGuildData> {
		return this.#mutate({
			method: 'POST',
			segments: ['internal', 'guilds', guildId, 'setup'],
			body: input,
			requestSchema: setupGuildRequestSchema,
			responseSchema: setupGuildDataSchema,
			context,
			idempotencyKey: context.idempotencyKey,
			logRequest: true
		});
	}

	syncDiscordCatalog(
		guildId: string,
		input: DiscordCatalogSyncRequest,
		context: ApiRequestContext = {}
	): Promise<DiscordCatalogSyncData> {
		return this.#mutate({
			method: 'PUT',
			segments: ['internal', 'guilds', guildId, 'discord-catalog'],
			body: input,
			requestSchema: discordCatalogSyncRequestSchema,
			responseSchema: discordCatalogSyncDataSchema,
			context,
			idempotencyKey: null,
			logRequest: false
		});
	}

	createRecruitment(
		guildId: string,
		input: CreateRecruitmentRequest,
		context: ApiMutationContext
	): Promise<RecruitmentSnapshot> {
		return this.#recruitmentMutation(
			'POST',
			['api', 'guilds', guildId, 'recruitments'],
			input,
			createRecruitmentRequestSchema,
			context
		);
	}

	bindRecruitmentMessage(
		guildId: string,
		recruitmentId: string,
		input: BindRecruitmentMessageRequest,
		context: ApiMutationContext
	): Promise<RecruitmentSnapshot> {
		return this.#recruitmentMutation(
			'PUT',
			['api', 'guilds', guildId, 'recruitments', recruitmentId, 'message'],
			input,
			bindRecruitmentMessageRequestSchema,
			context
		);
	}

	joinRecruitment(
		guildId: string,
		recruitmentId: string,
		input: JoinRecruitmentRequest,
		context: ApiMutationContext
	): Promise<RecruitmentSnapshot> {
		return this.#recruitmentMutation(
			'POST',
			['api', 'guilds', guildId, 'recruitments', recruitmentId, 'members'],
			input,
			joinRecruitmentRequestSchema,
			context
		);
	}

	leaveRecruitment(
		guildId: string,
		recruitmentId: string,
		userId: string,
		input: ActorRequest,
		context: ApiMutationContext
	): Promise<RecruitmentSnapshot> {
		return this.#recruitmentMutation(
			'DELETE',
			['api', 'guilds', guildId, 'recruitments', recruitmentId, 'members', userId],
			input,
			actorRequestSchema,
			context
		);
	}

	setPreferredLanes(
		guildId: string,
		recruitmentId: string,
		userId: string,
		input: SetPreferredLanesRequest,
		context: ApiMutationContext
	): Promise<RecruitmentSnapshot> {
		return this.#recruitmentMutation(
			'PUT',
			[
				'api',
				'guilds',
				guildId,
				'recruitments',
				recruitmentId,
				'members',
				userId,
				'preferred-lanes'
			],
			input,
			setPreferredLanesRequestSchema,
			context
		);
	}

	startEntry(
		guildId: string,
		recruitmentId: string,
		input: ActorRequest,
		context: ApiMutationContext
	): Promise<RecruitmentSnapshot> {
		return this.#recruitmentMutation(
			'POST',
			['api', 'guilds', guildId, 'recruitments', recruitmentId, 'close'],
			input,
			actorRequestSchema,
			context
		);
	}

	#recruitmentMutation<TRequest>(
		method: 'POST' | 'PUT' | 'DELETE',
		segments: string[],
		body: TRequest,
		requestSchema: ZodType<TRequest>,
		context: ApiMutationContext
	): Promise<RecruitmentSnapshot> {
		return this.#mutate({
			method,
			segments,
			body,
			requestSchema,
			responseSchema: recruitmentSnapshotSchema,
			context,
			idempotencyKey: context.idempotencyKey,
			logRequest: true
		});
	}

	async #mutate<TRequest, TResponse>(
		request: MutationRequest<TRequest, TResponse>
	): Promise<TResponse> {
		const idempotencyKey = request.idempotencyKey?.trim() ?? null;
		if (request.idempotencyKey !== null && (!idempotencyKey || idempotencyKey.length > 255)) {
			throw new MonkeyBotApiContractError('idempotencyKey must contain 1 to 255 characters');
		}
		const body = request.requestSchema.safeParse(request.body);
		if (!body.success) {
			const fields = body.error.issues
				.map((issue) => issue.path.map(String).join('.'))
				.filter(Boolean);
			throw new MonkeyBotApiContractError(
				`request does not match the API contract${fields.length ? `: ${[...new Set(fields)].join(', ')}` : ''}`
			);
		}

		const url = this.#url(request.segments);
		const controller = new AbortController();
		const timeout = setTimeout(
			() => controller.abort(new Error('API request timed out')),
			this.#timeoutMs
		);
		timeout.unref();
		const abortFromCaller = () => controller.abort(request.context.signal?.reason);
		request.context.signal?.addEventListener('abort', abortFromCaller, { once: true });

		let response: Response;
		try {
			const headers = new Headers({
				accept: 'application/json',
				'content-type': 'application/json',
				authorization: `Bearer ${this.#serviceToken}`
			});
			if (idempotencyKey) headers.set('idempotency-key', idempotencyKey);
			response = await this.#fetch(url, {
				method: request.method,
				headers,
				body: JSON.stringify(body.data),
				redirect: 'error',
				signal: controller.signal
			});
		} catch (error) {
			const timedOut = controller.signal.aborted && !request.context.signal?.aborted;
			if (request.logRequest) {
				this.#logger?.warn(
					{ err: error, method: request.method, path: url.pathname, timedOut },
					'bot API request failed'
				);
			}
			throw new MonkeyBotApiTransportError(
				timedOut ? 'Bot API request timed out' : 'Bot API request failed',
				{ cause: error }
			);
		} finally {
			clearTimeout(timeout);
			request.context.signal?.removeEventListener('abort', abortFromCaller);
		}

		const payload = await readJson(response);
		if (!response.ok) {
			const parsed = apiErrorEnvelopeSchema.safeParse(payload);
			const requestId = parsed.success
				? parsed.data.error.requestId
				: response.headers.get('x-request-id');
			if (request.logRequest) {
				this.#logger?.warn(
					{
						method: request.method,
						path: url.pathname,
						status: response.status,
						requestId,
						code: parsed.success ? parsed.data.error.code : 'invalid_error_response'
					},
					'bot API rejected request'
				);
			}
			throw new MonkeyBotApiError({
				status: response.status,
				code: parsed.success ? parsed.data.error.code : 'invalid_error_response',
				message: parsed.success ? parsed.data.error.message : 'Bot API rejected the request',
				requestId,
				details: parsed.success ? parsed.data.error.details : undefined
			});
		}

		const envelope = successEnvelopeSchema(request.responseSchema).safeParse(payload);
		if (!envelope.success) {
			if (request.logRequest) {
				this.#logger?.error(
					{ method: request.method, path: url.pathname, status: response.status },
					'bot API returned an invalid success payload'
				);
			}
			throw new MonkeyBotApiContractError('Bot API response does not match the expected contract');
		}
		if (request.logRequest) {
			this.#logger?.debug(
				{
					method: request.method,
					path: url.pathname,
					status: response.status,
					requestId: envelope.data.meta.requestId
				},
				'bot API request completed'
			);
		}
		return envelope.data.data;
	}

	#url(segments: string[]): URL {
		const url = new URL(this.#baseUrl);
		const prefix = url.pathname.replace(/\/+$/, '');
		url.pathname = `${prefix}/${segments.map((segment) => encodeURIComponent(segment)).join('/')}`;
		return url;
	}
}

function normalizeBaseUrl(value: URL | string): URL {
	const url = new URL(value);
	if (
		(url.protocol !== 'http:' && url.protocol !== 'https:') ||
		url.username ||
		url.password ||
		url.search ||
		url.hash
	) {
		throw new MonkeyBotApiContractError('baseUrl must be an HTTP origin or base path');
	}
	url.pathname = url.pathname.replace(/\/+$/, '') || '/';
	return url;
}

async function readJson(response: Response): Promise<unknown> {
	try {
		return await response.json();
	} catch (error) {
		throw new MonkeyBotApiContractError(
			`Bot API returned a non-JSON response with status ${response.status}`,
			{ cause: error }
		);
	}
}
