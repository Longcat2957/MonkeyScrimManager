import { DiscordAPIError, Events, type Client, type MessageEditOptions } from 'discord.js';
import type { Logger } from 'pino';

import {
	MonkeyBotApiError,
	MonkeyBotApiTransportError,
	type MonkeyBotApiClient
} from '../api/client.js';
import {
	auctionTournamentResultDeleteNotificationPayloadSchema,
	auctionTournamentResultNotificationPayloadSchema,
	auctionMatchLineupConfirmationNotificationPayloadSchema,
	auctionRecruitmentMessageNotificationPayloadSchema,
	entryConfirmationNotificationPayloadSchema,
	recruitmentMessageNotificationPayloadSchema,
	seriesResultNotificationPayloadSchema,
	type BotNotification,
	type NackNotificationRequest
} from '../api/contracts.js';
import { renderAuctionRecruitmentMessage } from './auction-recruitment-message.js';
import { renderAuctionMatchLineupConfirmationMessage } from './auction-match-lineup-confirmation-message.js';
import { renderAuctionTournamentResultMessage } from './auction-tournament-result-message.js';
import { renderEntryConfirmationMessage } from './entry-confirmation-message.js';
import { renderRecruitmentMessage } from './recruitment-message.js';
import {
	renderSeriesResultMessage,
	type SeriesResultImageResolver
} from './series-result-message.js';

const DEFAULT_CLAIM_LIMIT = 20;

type NotificationApi = Pick<
	MonkeyBotApiClient,
	| 'acknowledgeNotification'
	| 'acknowledgeAuctionRecruitmentMessage'
	| 'acknowledgeRecruitmentMessage'
	| 'claimNotifications'
	| 'nackNotification'
>;

export interface BotNotificationRelay {
	poll(): Promise<void>;
	close(): void;
}

export interface BotNotificationRelayOptions {
	instanceId: string;
	webOrigin: URL;
	pollIntervalMs: number;
	claimLimit?: number;
	resultImageResolver?: SeriesResultImageResolver;
}

export function installBotNotificationRelay(
	client: Client,
	api: NotificationApi,
	logger: Logger,
	options: BotNotificationRelayOptions
): BotNotificationRelay {
	return new BotNotificationRelayManager(client, api, logger, options);
}

class BotNotificationRelayManager implements BotNotificationRelay {
	readonly #client: Client;
	readonly #api: NotificationApi;
	readonly #logger: Logger;
	readonly #instanceId: string;
	readonly #webOrigin: URL;
	readonly #pollIntervalMs: number;
	readonly #claimLimit: number;
	readonly #resultImageResolver: SeriesResultImageResolver | undefined;
	readonly #abortController = new AbortController();
	#timer: NodeJS.Timeout | null = null;
	#activePoll: Promise<void> | null = null;
	#closed = false;

	readonly #onClientReady = () => {
		if (this.#closed || this.#timer) return;
		void this.poll();
		this.#timer = setInterval(() => void this.poll(), this.#pollIntervalMs);
		this.#timer.unref();
	};

	constructor(
		client: Client,
		api: NotificationApi,
		logger: Logger,
		options: BotNotificationRelayOptions
	) {
		if (!/^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(options.instanceId)) {
			throw new Error('notification relay instanceId is invalid');
		}
		if (!Number.isInteger(options.pollIntervalMs) || options.pollIntervalMs < 250) {
			throw new Error('notification relay pollIntervalMs must be an integer of at least 250');
		}
		const claimLimit = options.claimLimit ?? DEFAULT_CLAIM_LIMIT;
		if (!Number.isInteger(claimLimit) || claimLimit < 1 || claimLimit > 50) {
			throw new Error('notification relay claimLimit must be between 1 and 50');
		}

		this.#client = client;
		this.#api = api;
		this.#logger = logger;
		this.#instanceId = options.instanceId;
		this.#webOrigin = options.webOrigin;
		this.#pollIntervalMs = options.pollIntervalMs;
		this.#claimLimit = claimLimit;
		this.#resultImageResolver = options.resultImageResolver;
		client.on(Events.ClientReady, this.#onClientReady);
	}

	poll(): Promise<void> {
		if (this.#closed) return Promise.resolve();
		if (this.#activePoll) return this.#activePoll;

		const operation = this.#pollOnce().finally(() => {
			if (this.#activePoll === operation) this.#activePoll = null;
		});
		this.#activePoll = operation;
		return operation;
	}

	close(): void {
		if (this.#closed) return;
		this.#closed = true;
		if (this.#timer) clearInterval(this.#timer);
		this.#timer = null;
		this.#abortController.abort(new Error('Bot notification relay is shutting down'));
		this.#client.off(Events.ClientReady, this.#onClientReady);
	}

	async #pollOnce(): Promise<void> {
		let notifications: BotNotification[];
		try {
			const claimed = await this.#api.claimNotifications(
				{ instanceId: this.#instanceId, limit: this.#claimLimit },
				{ signal: this.#abortController.signal }
			);
			notifications = claimed.notifications;
		} catch (error) {
			if (this.#closed && this.#abortController.signal.aborted) return;
			this.#logger.warn({ err: error }, 'Bot notification claim failed');
			return;
		}

		for (const notification of notifications) {
			if (this.#closed) return;
			await this.#deliver(notification);
		}
	}

	async #deliver(notification: BotNotification): Promise<void> {
		switch (notification.type) {
			case 'RECRUITMENT_MESSAGE_CREATE':
			case 'RECRUITMENT_MESSAGE_UPDATE':
			case 'RECRUITMENT_MESSAGE_DELETE': {
				const payload = recruitmentMessageNotificationPayloadSchema.safeParse(notification.payload);
				if (!payload.success) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message: 'Recruitment notification payload does not match schema version 1'
					});
					return;
				}

				switch (notification.type) {
					case 'RECRUITMENT_MESSAGE_CREATE':
						await this.#createRecruitmentMessage(notification, payload.data.snapshot);
						return;
					case 'RECRUITMENT_MESSAGE_UPDATE':
						await this.#updateRecruitmentMessage(notification, payload.data.snapshot);
						return;
					case 'RECRUITMENT_MESSAGE_DELETE':
						await this.#deleteRecruitmentMessage(notification, payload.data.snapshot);
						return;
				}
				return;
			}
			case 'AUCTION_RECRUITMENT_MESSAGE_CREATE':
			case 'AUCTION_RECRUITMENT_MESSAGE_UPDATE':
			case 'AUCTION_RECRUITMENT_MESSAGE_DELETE': {
				const payload = auctionRecruitmentMessageNotificationPayloadSchema.safeParse(
					notification.payload
				);
				if (!payload.success) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message: 'Auction recruitment notification payload does not match schema version 1 or 2'
					});
					return;
				}

				switch (notification.type) {
					case 'AUCTION_RECRUITMENT_MESSAGE_CREATE':
						await this.#createAuctionRecruitmentMessage(notification, payload.data.snapshot);
						return;
					case 'AUCTION_RECRUITMENT_MESSAGE_UPDATE':
						await this.#updateAuctionRecruitmentMessage(notification, payload.data.snapshot);
						return;
					case 'AUCTION_RECRUITMENT_MESSAGE_DELETE':
						await this.#deleteAuctionRecruitmentMessage(notification, payload.data.snapshot);
						return;
				}
				return;
			}
			case 'ENTRY_CONFIRMATION_CREATE': {
				const payload = entryConfirmationNotificationPayloadSchema.safeParse(notification.payload);
				if (!payload.success) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message: 'Entry confirmation payload does not match schema version 1'
					});
					return;
				}
				await this.#createEntryConfirmation(notification, payload.data.snapshot);
				return;
			}
			case 'AUCTION_MATCH_LINEUP_CONFIRMATION_CREATE': {
				const payload = auctionMatchLineupConfirmationNotificationPayloadSchema.safeParse(
					notification.payload
				);
				if (!payload.success) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message: 'Auction match lineup confirmation payload does not match schema version 1'
					});
					return;
				}
				await this.#createAuctionMatchLineupConfirmation(notification, payload.data.snapshot);
				return;
			}
			case 'SERIES_RESULT_CREATE': {
				const payload = seriesResultNotificationPayloadSchema.safeParse(notification.payload);
				if (!payload.success) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message:
							'Series result notification payload does not match supported schema versions 1 or 2'
					});
					return;
				}
				await this.#createSeriesResult(notification, payload.data.snapshot);
				return;
			}
			case 'AUCTION_TOURNAMENT_RESULT_CREATE': {
				const payload = auctionTournamentResultNotificationPayloadSchema.safeParse(
					notification.payload
				);
				if (!payload.success) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message:
							'Auction tournament result notification payload does not match schema version 1'
					});
					return;
				}
				if (
					payload.data.guildId !== notification.guildId ||
					payload.data.channelId !== notification.channelId
				) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message:
							'Auction tournament result notification routing does not match its claimed notification'
					});
					return;
				}
				await this.#createAuctionTournamentResult(notification, payload.data);
				return;
			}
			case 'AUCTION_TOURNAMENT_RESULT_DELETE': {
				const payload = auctionTournamentResultDeleteNotificationPayloadSchema.safeParse(
					notification.payload
				);
				if (!payload.success) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message: 'Auction tournament result deletion payload does not match schema version 1'
					});
					return;
				}
				if (
					payload.data.guildId !== notification.guildId ||
					payload.data.channelId !== notification.channelId
				) {
					await this.#nack(notification, {
						kind: 'HTTP',
						status: 422,
						message:
							'Auction tournament result deletion routing does not match its claimed notification'
					});
					return;
				}
				await this.#deleteAuctionTournamentResult(notification, payload.data);
				return;
			}
			default:
				await this.#nack(notification, {
					kind: 'HTTP',
					status: 422,
					message: `Unsupported bot notification type: ${notification.type}`
				});
		}
	}

	async #createSeriesResult(
		notification: BotNotification,
		snapshot: ReturnType<typeof seriesResultNotificationPayloadSchema.parse>['snapshot']
	): Promise<void> {
		let messageId: string;
		try {
			const channel = await this.#sendableGuildChannel(notification.channelId);
			const messageOptions = await renderSeriesResultMessage(
				snapshot,
				this.#webOrigin,
				this.#resultImageResolver ? { resolveImages: this.#resultImageResolver } : {}
			);
			const message = await channel.send({
				...messageOptions,
				nonce: notification.discordNonce,
				enforceNonce: true
			});
			messageId = message.id;
		} catch (error) {
			await this.#nack(notification, failureFor(error));
			return;
		}

		try {
			await this.#acknowledge(notification, messageId);
			this.#logger.info(
				{
					notificationId: notification.id,
					guildId: notification.guildId,
					channelId: notification.channelId,
					messageId
				},
				'Series result Discord message created'
			);
		} catch (error) {
			// The stable Discord nonce makes a delivery retry safe when the API acknowledgement is lost.
			await this.#nack(notification, failureFor(error));
		}
	}

	async #createAuctionTournamentResult(
		notification: BotNotification,
		payload: ReturnType<typeof auctionTournamentResultNotificationPayloadSchema.parse>
	): Promise<void> {
		let messageId: string;
		try {
			const channel = await this.#sendableGuildChannel(notification.channelId);
			const message = await channel.send({
				...renderAuctionTournamentResultMessage(payload, this.#webOrigin),
				nonce: notification.discordNonce,
				enforceNonce: true
			});
			messageId = message.id;
		} catch (error) {
			await this.#nack(notification, failureFor(error));
			return;
		}

		try {
			await this.#acknowledge(notification, messageId);
			this.#logger.info(
				{
					notificationId: notification.id,
					guildId: notification.guildId,
					channelId: notification.channelId,
					messageId,
					tournamentId: payload.tournamentId
				},
				'Auction tournament result Discord message created'
			);
		} catch (error) {
			// The stable Discord nonce makes a delivery retry safe when the API acknowledgement is lost.
			await this.#nack(notification, failureFor(error));
		}
	}

	async #deleteAuctionTournamentResult(
		notification: BotNotification,
		payload: ReturnType<typeof auctionTournamentResultDeleteNotificationPayloadSchema.parse>
	): Promise<void> {
		const messageId = payload.discordMessageId;
		try {
			const message = await this.#recruitmentMessage(notification.channelId, messageId);
			await message.delete();
		} catch (error) {
			if (!isUnknownDiscordMessage(error)) {
				await this.#nack(notification, failureFor(error));
				return;
			}
		}

		try {
			await this.#acknowledge(notification, messageId);
		} catch (error) {
			await this.#nack(notification, failureFor(error));
		}
	}

	async #createEntryConfirmation(
		notification: BotNotification,
		snapshot: ReturnType<typeof entryConfirmationNotificationPayloadSchema.parse>['snapshot']
	): Promise<void> {
		let messageId: string;
		try {
			const channel = await this.#sendableGuildChannel(notification.channelId);
			const message = await channel.send({
				...renderEntryConfirmationMessage(snapshot, this.#webOrigin),
				nonce: notification.discordNonce,
				enforceNonce: true
			});
			messageId = message.id;
		} catch (error) {
			await this.#nack(notification, failureFor(error));
			return;
		}

		try {
			await this.#acknowledge(notification, messageId);
			this.#logger.info(
				{
					notificationId: notification.id,
					guildId: notification.guildId,
					channelId: notification.channelId,
					messageId
				},
				'Entry confirmation Discord message created'
			);
		} catch (error) {
			// The stable Discord nonce makes a delivery retry safe when the API acknowledgement is lost.
			await this.#nack(notification, failureFor(error));
		}
	}

	async #createAuctionMatchLineupConfirmation(
		notification: BotNotification,
		snapshot: ReturnType<
			typeof auctionMatchLineupConfirmationNotificationPayloadSchema.parse
		>['snapshot']
	): Promise<void> {
		let messageId: string;
		try {
			const channel = await this.#sendableGuildChannel(notification.channelId);
			const message = await channel.send({
				...renderAuctionMatchLineupConfirmationMessage(snapshot, this.#webOrigin),
				nonce: notification.discordNonce,
				enforceNonce: true
			});
			messageId = message.id;
		} catch (error) {
			await this.#nack(notification, failureFor(error));
			return;
		}

		try {
			await this.#acknowledge(notification, messageId);
			this.#logger.info(
				{
					notificationId: notification.id,
					guildId: notification.guildId,
					channelId: notification.channelId,
					messageId,
					matchId: snapshot.match.id
				},
				'Auction match lineup confirmation Discord message created'
			);
		} catch (error) {
			// The stable Discord nonce makes a delivery retry safe when the API acknowledgement is lost.
			await this.#nack(notification, failureFor(error));
		}
	}

	async #createRecruitmentMessage(
		notification: BotNotification,
		snapshot: ReturnType<typeof recruitmentMessageNotificationPayloadSchema.parse>['snapshot']
	): Promise<void> {
		let messageId: string;
		try {
			const channel = await this.#sendableGuildChannel(notification.channelId);
			const message = await channel.send({
				...renderRecruitmentMessage(snapshot, this.#webOrigin),
				nonce: notification.discordNonce,
				enforceNonce: true
			});
			messageId = message.id;
		} catch (error) {
			await this.#nack(notification, failureFor(error));
			return;
		}

		try {
			await this.#api.acknowledgeRecruitmentMessage(
				{
					instanceId: this.#instanceId,
					notificationId: notification.id,
					leaseToken: notification.leaseToken,
					discordMessageId: messageId
				},
				{ signal: this.#abortController.signal }
			);
			this.#logger.info(
				{
					notificationId: notification.id,
					guildId: notification.guildId,
					channelId: notification.channelId,
					messageId
				},
				'Recruitment Discord message created'
			);
		} catch (error) {
			// The stable Discord nonce makes a delivery retry safe when the API acknowledgement is lost.
			await this.#nack(notification, failureFor(error));
		}
	}

	async #updateRecruitmentMessage(
		notification: BotNotification,
		snapshot: ReturnType<typeof recruitmentMessageNotificationPayloadSchema.parse>['snapshot']
	): Promise<void> {
		const messageId = snapshot.recruitment.messageId;
		if (!messageId) {
			await this.#nack(notification, {
				kind: 'HTTP',
				status: 422,
				message: 'Recruitment message update is missing a Discord message ID'
			});
			return;
		}

		try {
			const message = await this.#recruitmentMessage(notification.channelId, messageId);
			await message.edit(messageEditOptions(snapshot, this.#webOrigin));
			await this.#acknowledge(notification, messageId);
		} catch (error) {
			await this.#nack(notification, failureFor(error));
		}
	}

	async #deleteRecruitmentMessage(
		notification: BotNotification,
		snapshot: ReturnType<typeof recruitmentMessageNotificationPayloadSchema.parse>['snapshot']
	): Promise<void> {
		const messageId = snapshot.recruitment.messageId;
		if (!messageId) {
			await this.#nack(notification, {
				kind: 'HTTP',
				status: 422,
				message: 'Recruitment message deletion is missing a Discord message ID'
			});
			return;
		}

		try {
			const message = await this.#recruitmentMessage(notification.channelId, messageId);
			await message.delete();
		} catch (error) {
			if (!isUnknownDiscordMessage(error)) {
				await this.#nack(notification, failureFor(error));
				return;
			}
		}

		try {
			await this.#acknowledge(notification, messageId);
		} catch (error) {
			await this.#nack(notification, failureFor(error));
		}
	}

	async #createAuctionRecruitmentMessage(
		notification: BotNotification,
		snapshot: ReturnType<
			typeof auctionRecruitmentMessageNotificationPayloadSchema.parse
		>['snapshot']
	): Promise<void> {
		let messageId: string;
		try {
			const channel = await this.#sendableGuildChannel(notification.channelId);
			const message = await channel.send({
				...renderAuctionRecruitmentMessage(snapshot, this.#webOrigin),
				nonce: notification.discordNonce,
				enforceNonce: true
			});
			messageId = message.id;
		} catch (error) {
			await this.#nack(notification, failureFor(error));
			return;
		}

		try {
			await this.#api.acknowledgeAuctionRecruitmentMessage(
				{
					instanceId: this.#instanceId,
					notificationId: notification.id,
					leaseToken: notification.leaseToken,
					discordMessageId: messageId
				},
				{ signal: this.#abortController.signal }
			);
			this.#logger.info(
				{
					notificationId: notification.id,
					guildId: notification.guildId,
					channelId: notification.channelId,
					messageId
				},
				'Auction recruitment Discord message created'
			);
		} catch (error) {
			await this.#nack(notification, failureFor(error));
		}
	}

	async #updateAuctionRecruitmentMessage(
		notification: BotNotification,
		snapshot: ReturnType<
			typeof auctionRecruitmentMessageNotificationPayloadSchema.parse
		>['snapshot']
	): Promise<void> {
		const messageId = snapshot.recruitment.messageId;
		if (!messageId) {
			await this.#nack(notification, {
				kind: 'HTTP',
				status: 422,
				message: 'Auction recruitment message update is missing a Discord message ID'
			});
			return;
		}

		try {
			const message = await this.#recruitmentMessage(notification.channelId, messageId);
			const rendered = renderAuctionRecruitmentMessage(snapshot, this.#webOrigin);
			await message.edit({ embeds: rendered.embeds, components: rendered.components });
			await this.#acknowledge(notification, messageId);
		} catch (error) {
			await this.#nack(notification, failureFor(error));
		}
	}

	async #deleteAuctionRecruitmentMessage(
		notification: BotNotification,
		snapshot: ReturnType<
			typeof auctionRecruitmentMessageNotificationPayloadSchema.parse
		>['snapshot']
	): Promise<void> {
		const messageId = snapshot.recruitment.messageId;
		if (!messageId) {
			await this.#nack(notification, {
				kind: 'HTTP',
				status: 422,
				message: 'Auction recruitment message deletion is missing a Discord message ID'
			});
			return;
		}

		try {
			const message = await this.#recruitmentMessage(notification.channelId, messageId);
			await message.delete();
		} catch (error) {
			if (!isUnknownDiscordMessage(error)) {
				await this.#nack(notification, failureFor(error));
				return;
			}
		}

		try {
			await this.#acknowledge(notification, messageId);
		} catch (error) {
			await this.#nack(notification, failureFor(error));
		}
	}

	async #acknowledge(notification: BotNotification, discordMessageId: string): Promise<void> {
		await this.#api.acknowledgeNotification(
			{
				instanceId: this.#instanceId,
				notificationId: notification.id,
				leaseToken: notification.leaseToken,
				discordMessageId
			},
			{ signal: this.#abortController.signal }
		);
	}

	async #sendableGuildChannel(channelId: string) {
		const channel = await this.#client.channels.fetch(channelId);
		if (!channel || !channel.isSendable() || channel.isDMBased()) {
			throw new NotificationHttpError(
				404,
				`Configured Discord channel ${channelId} is unavailable`
			);
		}
		return channel;
	}

	async #recruitmentMessage(channelId: string, messageId: string) {
		const channel = await this.#client.channels.fetch(channelId);
		if (!channel || !channel.isTextBased() || channel.isDMBased()) {
			throw new NotificationHttpError(
				404,
				`Configured recruitment channel ${channelId} is unavailable`
			);
		}
		return channel.messages.fetch(messageId);
	}

	async #nack(
		notification: BotNotification,
		failure: NackNotificationRequest['failure']
	): Promise<void> {
		try {
			const outcome = await this.#api.nackNotification(
				{
					instanceId: this.#instanceId,
					notificationId: notification.id,
					leaseToken: notification.leaseToken,
					failure
				},
				{ signal: this.#abortController.signal }
			);
			this.#logger.warn(
				{
					notificationId: notification.id,
					guildId: notification.guildId,
					type: notification.type,
					failure: failure.kind,
					state: outcome.state
				},
				'Bot notification delivery did not complete'
			);
		} catch (error) {
			if (this.#closed && this.#abortController.signal.aborted) return;
			this.#logger.error(
				{ err: error, notificationId: notification.id, guildId: notification.guildId },
				'Bot notification nack failed'
			);
		}
	}
}

function messageEditOptions(
	snapshot: ReturnType<typeof recruitmentMessageNotificationPayloadSchema.parse>['snapshot'],
	webOrigin: URL
): MessageEditOptions {
	const rendered = renderRecruitmentMessage(snapshot, webOrigin);
	return {
		...(rendered.embeds ? { embeds: rendered.embeds } : {}),
		...(rendered.components ? { components: rendered.components } : {})
	};
}

function isUnknownDiscordMessage(error: unknown): boolean {
	return error instanceof DiscordAPIError && error.code === 10_008;
}

function failureFor(error: unknown): NackNotificationRequest['failure'] {
	if (error instanceof NotificationHttpError) {
		return { kind: 'HTTP', status: error.status, message: error.message };
	}
	if (error instanceof DiscordAPIError || error instanceof MonkeyBotApiError) {
		const status = error.status;
		if (status === 429) {
			return {
				kind: 'HTTP',
				status,
				retryAfterMs: retryAfterMs(error),
				message: errorMessage(error)
			};
		}
		return { kind: 'HTTP', status, message: errorMessage(error) };
	}
	if (error instanceof MonkeyBotApiTransportError) {
		return { kind: 'TRANSPORT', message: errorMessage(error) };
	}
	return { kind: 'TRANSPORT', message: errorMessage(error) };
}

function retryAfterMs(error: DiscordAPIError | MonkeyBotApiError): number | undefined {
	if (!(error instanceof DiscordAPIError)) return undefined;
	const rawError = error.rawError;
	if (
		typeof rawError !== 'object' ||
		rawError === null ||
		!('retry_after' in rawError) ||
		typeof rawError.retry_after !== 'number' ||
		!Number.isFinite(rawError.retry_after)
	) {
		return undefined;
	}
	const milliseconds = Math.ceil(rawError.retry_after * 1_000);
	return milliseconds > 0 && milliseconds <= 3_600_000 ? milliseconds : undefined;
}

function errorMessage(error: unknown): string {
	const message = error instanceof Error ? error.message : 'unknown notification delivery failure';
	return message.slice(0, 1_024);
}

class NotificationHttpError extends Error {
	constructor(
		readonly status: number,
		message: string
	) {
		super(message);
		this.name = 'NotificationHttpError';
	}
}
