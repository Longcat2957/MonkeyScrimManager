import { EventEmitter } from 'node:events';

import { DiscordAPIError, type Client } from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import { MonkeyBotApiTransportError, type MonkeyBotApiClient } from '../api/client.js';
import type {
	AuctionTournamentResultDeleteNotificationPayload,
	AuctionTournamentResultNotificationPayload,
	BotNotification,
	EntryConfirmationSnapshot,
	RecruitmentSnapshot,
	SeriesResultNotificationSnapshot,
	SeriesResultNotificationV1Snapshot,
	SeriesResultNotificationV2Snapshot
} from '../api/contracts.js';
import { auctionRecruitmentMessageNotificationPayloadSchema } from '../api/contracts.js';
import { renderAuctionRecruitmentMessage } from './auction-recruitment-message.js';
import { renderAuctionTournamentResultMessage } from './auction-tournament-result-message.js';
import {
	renderEntryConfirmationMessage,
	renderEntryConfirmationPng,
	renderEntryConfirmationSvg
} from './entry-confirmation-message.js';
import { renderRecruitmentMessage } from './recruitment-message.js';
import { installBotNotificationRelay } from './notification-relay.js';
import {
	renderSeriesResultMessage,
	type SeriesResultImageResolver
} from './series-result-message.js';

const guildId = '200000000000000001';
const channelId = '500000000000000001';
const messageId = '600000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const notificationId = '01900000-0000-7000-8000-000000000301';
const leaseToken = '01900000-0000-7000-8000-000000000302';
const instanceId = 'bot-test';
const discordNonce = 'abcdefghijklmnopqrstuv';
const webOrigin = new URL('https://dev.monkeybot.lol');
const noOpResultImageResolver: SeriesResultImageResolver = async () => undefined;

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

	it('claims an auction CREATE notification, sends its card, and uses the auction typed acknowledgement', async () => {
		const snapshot = auctionRecruitmentSnapshot();
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
		const current = createRelay(
			[auctionNotification('AUCTION_RECRUITMENT_MESSAGE_CREATE', snapshot)],
			channel
		);

		await current.relay.poll();

		expect(send).toHaveBeenCalledTimes(1);
		const sentOptions = send.mock.calls[0]?.[0];
		expect(sentOptions).toMatchObject({ nonce: discordNonce, enforceNonce: true });
		expect(messagePayload(sentOptions)).toEqual(
			messagePayload(renderAuctionRecruitmentMessage(snapshot, webOrigin))
		);
		expect(current.acknowledgeAuctionRecruitmentMessage).toHaveBeenCalledWith(
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

	it('normalizes a queued v1 auction notification that predates series rules', () => {
		const current = auctionRecruitmentSnapshot();
		const { rules, ...legacyRecruitment } = current.recruitment;
		void rules;

		const parsed = auctionRecruitmentMessageNotificationPayloadSchema.parse({
			schemaVersion: 1,
			auctionRecruitmentId: current.recruitment.id,
			snapshot: { ...current, recruitment: legacyRecruitment }
		});

		expect(parsed.snapshot.recruitment.rules).toEqual({
			hardFearless: false,
			semifinalBestOf: null,
			finalBestOf: 3
		});
	});

	it('sends an entry confirmation with its stable nonce and uses generic acknowledgement', async () => {
		const snapshot = entryConfirmationSnapshot();
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
		const current = createRelay([entryConfirmationNotification(snapshot)], channel);

		await current.relay.poll();

		expect(send).toHaveBeenCalledTimes(1);
		const sentOptions = send.mock.calls[0]?.[0];
		expect(sentOptions).toMatchObject({ nonce: discordNonce, enforceNonce: true });
		expect(messagePayload(sentOptions)).toEqual(
			messagePayload(renderEntryConfirmationMessage(snapshot, webOrigin))
		);
		const entryOptions = sentOptions as {
			embeds?: unknown;
			files?: Array<{ attachment: unknown; name: string | null }>;
		};
		expect(entryOptions.embeds).toBeUndefined();
		expect(entryOptions.files).toHaveLength(1);
		expect(entryOptions.files?.[0]).toMatchObject({ name: 'series-12-entry.png' });
		expect(entryOptions.files?.[0]?.attachment).toBeInstanceOf(Buffer);
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

	it('renders the entry roster as an escaped SVG-derived PNG card', () => {
		const snapshot = entryConfirmationSnapshot();
		snapshot.teams[0]!.players[0]!.displayName = '<Longcat & Co>';

		const svg = renderEntryConfirmationSvg(snapshot);
		const png = renderEntryConfirmationPng(snapshot);

		expect(svg).toContain('&lt;Longcat &amp; Co&gt;');
		expect(svg).not.toContain('<Longcat & Co>');
		expect(png.subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a');
	});

	it('sends a full schema v2 series result card with its stable nonce and generic acknowledgement', async () => {
		const snapshot = seriesResultV2Snapshot();
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
		const current = createRelay([seriesResultNotification(snapshot)], channel);

		await current.relay.poll();

		expect(send).toHaveBeenCalledTimes(1);
		const sentOptions = send.mock.calls[0]?.[0];
		expect(sentOptions).toMatchObject({ nonce: discordNonce, enforceNonce: true });
		expect(messagePayload(sentOptions)).toEqual(
			messagePayload(
				await renderSeriesResultMessage(snapshot, webOrigin, {
					resolveImages: noOpResultImageResolver
				})
			)
		);
		const resultOptions = sentOptions as {
			embeds?: unknown;
			files?: Array<{ attachment: unknown; name: string | null }>;
		};
		expect(resultOptions.embeds).toBeUndefined();
		expect(resultOptions.files).toHaveLength(1);
		expect(resultOptions.files?.[0]).toMatchObject({ name: 'series-12-result.png' });
		expect(resultOptions.files?.[0]?.attachment).toBeInstanceOf(Buffer);
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

	it('continues delivering an existing compact schema v1 result notification', async () => {
		const snapshot = seriesResultSnapshot();
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
		const current = createRelay([seriesResultNotification(snapshot)], channel);

		await current.relay.poll();

		expect(send).toHaveBeenCalledTimes(1);
		expect(messagePayload(send.mock.calls[0]?.[0])).toEqual(
			messagePayload(
				await renderSeriesResultMessage(snapshot, webOrigin, {
					resolveImages: noOpResultImageResolver
				})
			)
		);
		expect(current.acknowledgeNotification).toHaveBeenCalledOnce();
		expect(current.nackNotification).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('sends a compact auction tournament result embed with a stable nonce and generic acknowledgement', async () => {
		const payload = auctionTournamentResultPayload();
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
		const current = createRelay([auctionTournamentResultNotification(payload)], channel);

		await current.relay.poll();

		expect(send).toHaveBeenCalledTimes(1);
		const sentOptions = send.mock.calls[0]?.[0];
		expect(sentOptions).toMatchObject({ nonce: discordNonce, enforceNonce: true });
		expect(messagePayload(sentOptions)).toEqual(
			messagePayload(renderAuctionTournamentResultMessage(payload, webOrigin))
		);
		const resultOptions = sentOptions as {
			embeds?: unknown;
			files?: unknown;
			components?: unknown;
		};
		expect(resultOptions.embeds).toHaveLength(1);
		expect(resultOptions.files).toBeUndefined();
		expect(resultOptions.components).toHaveLength(1);
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

	it('nacks an auction tournament result whose payload routing differs from the claimed notification', async () => {
		const payload = auctionTournamentResultPayload();
		payload.channelId = '500000000000000002';
		const current = createRelay([auctionTournamentResultNotification(payload)], null);

		await current.relay.poll();

		expect(current.nackNotification).toHaveBeenCalledWith(
			expect.objectContaining({
				failure: {
					kind: 'HTTP',
					status: 422,
					message:
						'Auction tournament result notification routing does not match its claimed notification'
				}
			}),
			expect.any(Object)
		);
		expect(current.acknowledgeNotification).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('acknowledges auction result DELETE when Discord already reports unknown message 10008', async () => {
		const payload = auctionTournamentResultDeletePayload();
		const remove = vi.fn(async () => {
			throw unknownDiscordMessage();
		});
		const channel = textChannel({ delete: remove });
		const current = createRelay([auctionTournamentResultDeleteNotification(payload)], channel);

		await current.relay.poll();

		expect(channel.messages.fetch).toHaveBeenCalledWith(messageId);
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

	it('nacks an invalid series result payload as a terminal HTTP 422 failure', async () => {
		const snapshot = seriesResultSnapshot();
		const current = createRelay(
			[
				seriesResultNotification(snapshot, {
					schemaVersion: 1,
					seriesId: snapshot.series.id,
					snapshot: { ...snapshot, score: { team1: 2, team2: 1 } }
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
					message:
						'Series result notification payload does not match supported schema versions 1 or 2'
				}
			}),
			expect.any(Object)
		);
		expect(current.acknowledgeNotification).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('nacks a malformed full schema v2 result payload before Discord delivery', async () => {
		const snapshot = seriesResultV2Snapshot();
		snapshot.games[0]!.bans.blue.pop();
		const current = createRelay([seriesResultNotification(snapshot)], null);

		await current.relay.poll();

		expect(current.nackNotification).toHaveBeenCalledWith(
			expect.objectContaining({
				failure: {
					kind: 'HTTP',
					status: 422,
					message:
						'Series result notification payload does not match supported schema versions 1 or 2'
				}
			}),
			expect.any(Object)
		);
		expect(current.acknowledgeNotification).not.toHaveBeenCalled();
		current.relay.close();
	});

	it('retries a series result notification when its generic acknowledgement is unavailable', async () => {
		const snapshot = seriesResultSnapshot();
		const channel = {
			isSendable: () => true,
			isDMBased: () => false,
			send: vi.fn(async () => ({ id: messageId }))
		};
		const current = createRelay([seriesResultNotification(snapshot)], channel);
		current.acknowledgeNotification.mockRejectedValueOnce(
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
			bestOf: 3,
			hardFearless: false,
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

type AuctionRecruitmentSnapshot = ReturnType<
	typeof auctionRecruitmentMessageNotificationPayloadSchema.parse
>['snapshot'];

function auctionRecruitmentSnapshot(
	overrides: Partial<AuctionRecruitmentSnapshot['recruitment']> = {}
): AuctionRecruitmentSnapshot {
	return {
		recruitment: {
			id: recruitmentId,
			displayNumber: 51,
			title: '금요 경매 내전',
			format: 'TEN_PERSON',
			rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
			participantCount: 10,
			joinedParticipantCount: 1,
			teamCount: 2,
			status: 'OPEN',
			revision: 1,
			channelId: null,
			messageId: null,
			createdById: '100000000000000001',
			updatedAt: '2026-07-13T10:00:00Z',
			href: `/g/${guildId}/auction/recruitments/${recruitmentId}`,
			...overrides
		},
		participants: [{ id: '100000000000000001', displayName: 'Longcat', declaredLane: 'MID' }],
		links: { draft: null },
		permissions: { canManageMembers: false, canClose: true, canCancel: true }
	};
}

function auctionNotification(type: string, snapshot: AuctionRecruitmentSnapshot): BotNotification {
	return {
		id: notificationId,
		guildId,
		leaseToken,
		leaseUntil: '2026-07-13T10:00:30Z',
		channelId,
		type,
		payload: {
			schemaVersion: 2,
			auctionRecruitmentId: snapshot.recruitment.id,
			snapshot
		},
		discordNonce,
		attempts: 1
	};
}

function entryConfirmationSnapshot(): EntryConfirmationSnapshot {
	return {
		series: {
			id: '01900000-0000-7000-8000-000000000401',
			displayNumber: 12,
			title: '일반 내전 #12',
			bestOf: 3,
			hardFearless: true
		},
		teams: [
			{
				team: 'TEAM_1',
				name: '1팀',
				players: [
					{ id: '100000000000000001', displayName: 'Longcat', lane: 'TOP' },
					{ id: '100000000000000002', displayName: 'Jungle', lane: 'JUNGLE' },
					{ id: '100000000000000003', displayName: 'Mid', lane: 'MID' },
					{ id: '100000000000000004', displayName: 'Bottom', lane: 'BOTTOM' },
					{ id: '100000000000000005', displayName: 'Support', lane: 'SUPPORT' }
				]
			},
			{
				team: 'TEAM_2',
				name: '2팀',
				players: [
					{ id: '100000000000000006', displayName: 'Top Two', lane: 'TOP' },
					{ id: '100000000000000007', displayName: 'Jungle Two', lane: 'JUNGLE' },
					{ id: '100000000000000008', displayName: 'Mid Two', lane: 'MID' },
					{ id: '100000000000000009', displayName: 'Bottom Two', lane: 'BOTTOM' },
					{ id: '100000000000000010', displayName: 'Support Two', lane: 'SUPPORT' }
				]
			}
		],
		links: { pickBan: `/g/${guildId}/series/01900000-0000-7000-8000-000000000401/pick-ban` }
	};
}

function entryConfirmationNotification(snapshot: EntryConfirmationSnapshot): BotNotification {
	return {
		id: notificationId,
		guildId,
		leaseToken,
		leaseUntil: '2026-07-13T10:00:30Z',
		channelId,
		type: 'ENTRY_CONFIRMATION_CREATE',
		payload: {
			schemaVersion: 1,
			seriesId: snapshot.series.id,
			snapshot
		},
		discordNonce,
		attempts: 1
	};
}

function seriesResultSnapshot(): SeriesResultNotificationV1Snapshot {
	const seriesId = '01900000-0000-7000-8000-000000000401';
	return {
		series: {
			id: seriesId,
			displayNumber: 12,
			title: '일반 내전 #12',
			bestOf: 3,
			hardFearless: true,
			winningTeam: 'TEAM_1',
			terminalReason: 'SCORE_REACHED'
		},
		score: { team1: 2, team2: 0 },
		games: [
			{ setNumber: 1, winner: 'TEAM_1' },
			{ setNumber: 2, winner: 'TEAM_1' }
		],
		links: { result: `/g/${guildId}/series/${seriesId}/result` }
	};
}

function seriesResultV2Snapshot(): SeriesResultNotificationV2Snapshot {
	const seriesId = '01900000-0000-7000-8000-000000000401';
	const teams: SeriesResultNotificationV2Snapshot['teams'] = [
		{
			team: 'TEAM_1',
			name: '1팀',
			players: [
				{ id: '100000000000000001', displayName: 'Longcat', lane: 'TOP' },
				{ id: '100000000000000002', displayName: 'Jungle', lane: 'JUNGLE' },
				{ id: '100000000000000003', displayName: 'Mid', lane: 'MID' },
				{ id: '100000000000000004', displayName: 'Bottom', lane: 'BOTTOM' },
				{ id: '100000000000000005', displayName: 'Support', lane: 'SUPPORT' }
			]
		},
		{
			team: 'TEAM_2',
			name: '2팀',
			players: [
				{ id: '100000000000000006', displayName: 'Top Two', lane: 'TOP' },
				{ id: '100000000000000007', displayName: 'Jungle Two', lane: 'JUNGLE' },
				{ id: '100000000000000008', displayName: 'Mid Two', lane: 'MID' },
				{ id: '100000000000000009', displayName: 'Bottom Two', lane: 'BOTTOM' },
				{ id: '100000000000000010', displayName: 'Support Two', lane: 'SUPPORT' }
			]
		}
	];
	const team1 = teams[0]!;
	const team2 = teams[1]!;
	const game = (
		setNumber: number,
		winner: 'TEAM_1' | 'TEAM_2',
		sidePreset: 'DEFAULT' | 'SWAPPED'
	): SeriesResultNotificationV2Snapshot['games'][number] => ({
		setNumber,
		winner,
		sidePreset,
		team1: {
			team: team1.team,
			name: team1.name,
			picks: team1.players.map((player, index) => ({
				...player,
				championId: ['Garen', 'LeeSin', 'Ahri', 'Jinx', 'Lulu'][index]!
			}))
		},
		team2: {
			team: team2.team,
			name: team2.name,
			picks: team2.players.map((player, index) => ({
				...player,
				championId: ['Darius', 'Viego', 'Orianna', 'KaiSa', 'Nautilus'][index]!
			}))
		},
		bans: {
			blue: ['Yone', 'Rumble', 'Aatrox', 'Syndra', 'Draven'],
			red: ['Aurora', 'Sejuani', 'Azir', 'Aphelios', 'Rakan']
		},
		patchVersion: '15.14'
	});

	return {
		series: {
			id: seriesId,
			displayNumber: 12,
			title: '일반 내전 #12',
			bestOf: 3,
			hardFearless: true,
			winningTeam: 'TEAM_1',
			terminalReason: 'SCORE_REACHED'
		},
		teams: [...teams],
		score: { team1: 2, team2: 0 },
		games: [game(1, 'TEAM_1', 'DEFAULT'), game(2, 'TEAM_1', 'SWAPPED')],
		links: { result: `/g/${guildId}/series/${seriesId}/result` }
	};
}

function auctionTournamentResultPayload(): AuctionTournamentResultNotificationPayload {
	const tournamentId = '01900000-0000-7000-8000-000000000601';
	return {
		schemaVersion: 1,
		guildId,
		tournamentId,
		channelId,
		tournament: {
			displayNumber: 42,
			title: '금요 경매 내전',
			format: 'TWENTY_PERSON',
			semifinalBestOf: 1,
			finalBestOf: 3,
			hardFearless: true
		},
		champion: {
			teamId: '01900000-0000-7000-8000-000000000711',
			teamName: '블루팀'
		},
		matches: [
			{
				matchKey: 'SEMIFINAL_A',
				label: '준결승 A',
				bestOf: 1,
				leftTeamName: '블루팀',
				rightTeamName: '레드팀',
				leftScore: 1,
				rightScore: 0,
				winnerTeamName: '블루팀'
			},
			{
				matchKey: 'SEMIFINAL_B',
				label: '준결승 B',
				bestOf: 1,
				leftTeamName: '그린팀',
				rightTeamName: '퍼플팀',
				leftScore: 0,
				rightScore: 1,
				winnerTeamName: '퍼플팀'
			},
			{
				matchKey: 'FINAL',
				label: '결승',
				bestOf: 3,
				leftTeamName: '블루팀',
				rightTeamName: '퍼플팀',
				leftScore: 2,
				rightScore: 1,
				winnerTeamName: '블루팀'
			}
		],
		completedAt: '2026-07-14T12:00:00Z'
	};
}

function auctionTournamentResultNotification(
	payload: AuctionTournamentResultNotificationPayload
): BotNotification {
	return {
		id: notificationId,
		guildId,
		leaseToken,
		leaseUntil: '2026-07-13T10:00:30Z',
		channelId,
		type: 'AUCTION_TOURNAMENT_RESULT_CREATE',
		payload,
		discordNonce,
		attempts: 1
	};
}

function auctionTournamentResultDeletePayload(): AuctionTournamentResultDeleteNotificationPayload {
	return {
		schemaVersion: 1,
		guildId,
		tournamentId: '01900000-0000-7000-8000-000000000601',
		channelId,
		discordMessageId: messageId
	};
}

function auctionTournamentResultDeleteNotification(
	payload: AuctionTournamentResultDeleteNotificationPayload
): BotNotification {
	return {
		id: notificationId,
		guildId,
		leaseToken,
		leaseUntil: '2026-07-13T10:00:30Z',
		channelId,
		type: 'AUCTION_TOURNAMENT_RESULT_DELETE',
		payload,
		discordNonce,
		attempts: 1
	};
}

function seriesResultNotification(
	snapshot: SeriesResultNotificationSnapshot,
	payload: unknown = seriesResultPayload(snapshot)
): BotNotification {
	return {
		id: notificationId,
		guildId,
		leaseToken,
		leaseUntil: '2026-07-13T10:00:30Z',
		channelId,
		type: 'SERIES_RESULT_CREATE',
		payload,
		discordNonce,
		attempts: 1
	};
}

function seriesResultPayload(snapshot: SeriesResultNotificationSnapshot): unknown {
	return {
		schemaVersion: 'teams' in snapshot ? 2 : 1,
		seriesId: snapshot.series.id,
		snapshot
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
	const acknowledgeAuctionRecruitmentMessage = vi.fn(async () => ({
		acknowledged: true as const,
		deliveredAt: '2026-07-13T10:00:01Z',
		auctionRecruitment: { id: recruitmentId, revision: 2 }
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
		acknowledgeAuctionRecruitmentMessage,
		nackNotification
	} as unknown as Pick<
		MonkeyBotApiClient,
		| 'claimNotifications'
		| 'acknowledgeNotification'
		| 'acknowledgeAuctionRecruitmentMessage'
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
		pollIntervalMs: 1_000,
		resultImageResolver: noOpResultImageResolver
	});

	return {
		relay,
		claimNotifications,
		acknowledgeNotification,
		acknowledgeRecruitmentMessage,
		acknowledgeAuctionRecruitmentMessage,
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
