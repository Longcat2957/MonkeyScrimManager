import { EventEmitter } from 'node:events';

import { DiscordAPIError, type Client } from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import { MonkeyBotApiTransportError, type MonkeyBotApiClient } from '../api/client.js';
import type { BotNotification, RecruitmentSnapshot } from '../api/contracts.js';
import { renderRecruitmentMessage } from './recruitment-message.js';
import { installBotNotificationRelay } from './notification-relay.js';

const guildId = '200000000000000001';
const channelId = '500000000000000001';
const messageId = '600000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const notificationId = '01900000-0000-7000-8000-000000000301';
const leaseToken = '01900000-0000-7000-8000-000000000302';
const instanceId = 'bot-test';
const discordNonce = 'abcdefghijklmnopqrstuv';
const webOrigin = new URL('https://dev.monkeybot.lol');

describe('Bot notification relay', () => {
	it('claims a CREATE notification, sends the normal recruitment message with its stable nonce, and uses typed acknowledgement', async () => {
		const snapshot = recruitmentSnapshot();
		const sent = { id: messageId };
		const send = vi.fn(async (options: unknown) => {
			void options;
			return sent;
		});
		const channel = {
			isSendable: () => true,
			isDMBased: () => false,
			send
		};
		const current = createRelay([notification('RECRUITMENT_MESSAGE_CREATE', snapshot)], channel);

		await current.relay.poll();

		expect(current.claimNotifications).toHaveBeenCalledWith(
			{ instanceId, limit: 20 },
			expect.any(Object)
		);
		expect(send).toHaveBeenCalledTimes(1);
		const sentOptions = send.mock.calls[0]?.[0];
		expect(sentOptions).toMatchObject({ nonce: discordNonce, enforceNonce: true });
		expect(messagePayload(sentOptions)).toEqual(
			messagePayload(renderRecruitmentMessage(snapshot, webOrigin))
		);
		expect(current.acknowledgeRecruitmentMessage).toHaveBeenCalledWith(
			{
				instanceId,
				notificationId,
				leaseToken,
				discordMessageId: messageId
			},
			expect.any(Object)
		);
		expect(current.acknowledgeNotification).not.toHaveBeenCalled();
		expect(current.nackNotification).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('nacks an invalid recruitment payload as a terminal HTTP 422 failure', async () => {
		const current = createRelay(
			[
				notification('RECRUITMENT_MESSAGE_CREATE', recruitmentSnapshot(), {
					schemaVersion: 999
				})
			],
			null
		);

		await current.relay.poll();

		expect(current.nackNotification).toHaveBeenCalledWith(
			expect.objectContaining({
				instanceId,
				notificationId,
				leaseToken,
				failure: {
					kind: 'HTTP',
					status: 422,
					message: 'Recruitment notification payload does not match schema version 1'
				}
			}),
			expect.any(Object)
		);
		expect(current.acknowledgeNotification).not.toHaveBeenCalled();
		expect(current.acknowledgeRecruitmentMessage).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('retries a CREATE notification when Discord sends successfully but the typed acknowledgement is unavailable', async () => {
		const snapshot = recruitmentSnapshot();
		const channel = {
			isSendable: () => true,
			isDMBased: () => false,
			send: vi.fn(async () => ({ id: messageId }))
		};
		const current = createRelay([notification('RECRUITMENT_MESSAGE_CREATE', snapshot)], channel);
		current.acknowledgeRecruitmentMessage.mockRejectedValueOnce(
			new MonkeyBotApiTransportError('connection reset')
		);

		await current.relay.poll();

		expect(current.nackNotification).toHaveBeenCalledWith(
			expect.objectContaining({
				failure: { kind: 'TRANSPORT', message: 'connection reset' }
			}),
			expect.any(Object)
		);
		current.relay.close();
	});

	it('nacks an unsupported notification type as a terminal HTTP 422 failure', async () => {
		const current = createRelay([notification('UNSUPPORTED_TYPE', recruitmentSnapshot())], null);

		await current.relay.poll();

		expect(current.nackNotification).toHaveBeenCalledWith(
			expect.objectContaining({
				failure: {
					kind: 'HTTP',
					status: 422,
					message: 'Unsupported bot notification type: UNSUPPORTED_TYPE'
				}
			}),
			expect.any(Object)
		);
		expect(current.acknowledgeNotification).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('edits an existing recruitment message and uses generic acknowledgement for UPDATE', async () => {
		const snapshot = recruitmentSnapshot({ channelId, messageId });
		const edit = vi.fn(async (options: unknown) => {
			void options;
		});
		const message = { edit };
		const channel = textChannel(message);
		const current = createRelay([notification('RECRUITMENT_MESSAGE_UPDATE', snapshot)], channel);

		await current.relay.poll();

		expect(channel.messages.fetch).toHaveBeenCalledWith(messageId);
		expect(messagePayload(edit.mock.calls[0]?.[0])).toEqual(
			messagePayload(renderRecruitmentMessage(snapshot, webOrigin))
		);
		expect(current.acknowledgeNotification).toHaveBeenCalledWith(
			{
				instanceId,
				notificationId,
				leaseToken,
				discordMessageId: messageId
			},
			expect.any(Object)
		);
		expect(current.acknowledgeRecruitmentMessage).not.toHaveBeenCalled();
		expect(current.nackNotification).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('acknowledges DELETE when Discord already reports unknown message 10008', async () => {
		const snapshot = recruitmentSnapshot({ channelId, messageId });
		const remove = vi.fn(async () => {
			throw unknownDiscordMessage();
		});
		const channel = textChannel({ delete: remove });
		const current = createRelay([notification('RECRUITMENT_MESSAGE_DELETE', snapshot)], channel);

		await current.relay.poll();

		expect(remove).toHaveBeenCalledOnce();
		expect(current.acknowledgeNotification).toHaveBeenCalledWith(
			{
				instanceId,
				notificationId,
				leaseToken,
				discordMessageId: messageId
			},
			expect.any(Object)
		);
		expect(current.nackNotification).not.toHaveBeenCalled();
		current.relay.close();
	});
});

function recruitmentSnapshot(
	overrides: Partial<RecruitmentSnapshot['recruitment']> = {}
): RecruitmentSnapshot {
	return {
		recruitment: {
			id: recruitmentId,
			displayNumber: 214,
			title: '금요 정기 내전',
			status: 'OPEN',
			targetCount: 10,
			revision: 3,
			createdById: '100000000000000001',
			channelId: null,
			messageId: null,
			updatedAt: '2026-07-13T10:00:00Z',
			...overrides
		},
		participants: [{ id: '100000000000000001', displayName: 'Longcat', preferredLanes: ['MID'] }],
		links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
	};
}

function notification(
	type: string,
	snapshot: RecruitmentSnapshot,
	payload: unknown = {
		schemaVersion: 1,
		recruitmentId,
		snapshot
	}
): BotNotification {
	return {
		id: notificationId,
		guildId,
		leaseToken,
		leaseUntil: '2026-07-13T10:00:30Z',
		channelId,
		type,
		payload,
		discordNonce,
		attempts: 1
	};
}

function createRelay(notifications: BotNotification[], channel: object | null) {
	const claimNotifications = vi.fn(async () => ({ notifications }));
	const acknowledgeNotification = vi.fn(async () => ({
		acknowledged: true as const,
		deliveredAt: '2026-07-13T10:00:01Z'
	}));
	const acknowledgeRecruitmentMessage = vi.fn(async () => ({
		acknowledged: true as const,
		deliveredAt: '2026-07-13T10:00:01Z',
		recruitment: { id: recruitmentId, revision: 4 }
	}));
	const nackNotification = vi.fn(async () => ({
		state: 'FAILED' as const,
		failureCode: 'invalid_notification',
		attempts: 1
	}));
	const api = {
		claimNotifications,
		acknowledgeNotification,
		acknowledgeRecruitmentMessage,
		nackNotification
	} as unknown as Pick<
		MonkeyBotApiClient,
		| 'claimNotifications'
		| 'acknowledgeNotification'
		| 'acknowledgeRecruitmentMessage'
		| 'nackNotification'
	>;
	const client = new EventEmitter() as EventEmitter & {
		channels: { fetch: ReturnType<typeof vi.fn> };
	};
	client.channels = { fetch: vi.fn(async () => channel) };
	const logger = { info: vi.fn(), warn: vi.fn(), error: vi.fn() } as unknown as Logger;
	const relay = installBotNotificationRelay(client as unknown as Client, api, logger, {
		instanceId,
		webOrigin,
		pollIntervalMs: 1_000
	});

	return {
		relay,
		claimNotifications,
		acknowledgeNotification,
		acknowledgeRecruitmentMessage,
		nackNotification,
		client
	};
}

function textChannel(message: object) {
	return {
		isTextBased: () => true,
		isDMBased: () => false,
		messages: { fetch: vi.fn(async () => message) }
	};
}

function messagePayload(value: unknown) {
	const record = value as { embeds?: unknown; components?: unknown };
	return {
		...(record.embeds ? { embeds: JSON.parse(JSON.stringify(record.embeds)) } : {}),
		...(record.components ? { components: JSON.parse(JSON.stringify(record.components)) } : {})
	};
}

function unknownDiscordMessage(): DiscordAPIError {
	return new DiscordAPIError(
		{ code: 10_008, message: 'Unknown Message' },
		10_008,
		404,
		'DELETE',
		'https://discord.com/api/v10/channels/500000000000000001/messages/600000000000000001',
		{ body: undefined, files: undefined }
	);
}
