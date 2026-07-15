import { MessageFlags, type Interaction } from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import type { MonkeyBotApiClient } from '../api/client.js';
import type { AuctionRecruitmentSnapshot } from '../api/contracts.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import { auctionRecruitmentCustomId } from './custom-id.js';
import type { InteractionContext } from './context.js';
import { dispatchInteraction } from './dispatcher.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';

const snapshot: AuctionRecruitmentSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '금요 경매 내전',
		format: 'TEN_PERSON',
		rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
		participantCount: 10,
		joinedParticipantCount: 1,
		teamCount: 2,
		status: 'OPEN',
		revision: 1,
		channelId: '500000000000000001',
		messageId: '600000000000000001',
		createdById: userId,
		updatedAt: '2026-07-14T10:00:00Z',
		href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
	},
	participants: [{ id: userId, displayName: 'MSM Longcat', declaredLane: 'MID' }],
	links: { draft: null },
	permissions: { canManageMembers: false, canClose: false, canCancel: true }
};

function context(
	joinAuctionRecruitment: ReturnType<typeof vi.fn>,
	leaveAuctionRecruitment: ReturnType<typeof vi.fn>
): InteractionContext {
	return {
		api: { joinAuctionRecruitment, leaveAuctionRecruitment } as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
		logger: { error: vi.fn(), info: vi.fn(), warn: vi.fn() } as unknown as Logger
	};
}

function auctionButton(action: 'join' | 'leave') {
	const deferUpdate = vi.fn(async () => undefined);
	const editReply = vi.fn(async () => undefined);
	return {
		value: {
			id: interactionId,
			guildId,
			guild: {},
			member: { nick: 'MSM Longcat', roles: ['300000000000000001'] },
			user: { id: userId, username: 'longcat_login', globalName: 'Longcat', avatar: null },
			customId: auctionRecruitmentCustomId(action, recruitmentId),
			inCachedGuild: () => true,
			isChatInputCommand: () => false,
			isButton: () => true,
			isStringSelectMenu: () => false,
			isRepliable: () => true,
			deferUpdate,
			editReply,
			reply: vi.fn(async () => undefined),
			followUp: vi.fn(async () => undefined)
		} as unknown as Interaction,
		deferUpdate,
		editReply
	};
}

function auctionLaneSelect() {
	const deferUpdate = vi.fn(async () => undefined);
	const editReply = vi.fn(async () => undefined);
	return {
		value: {
			id: interactionId,
			guildId,
			guild: {},
			member: { nick: 'MSM Longcat', roles: ['300000000000000001'] },
			user: { id: userId, username: 'longcat_login', globalName: 'Longcat', avatar: null },
			customId: auctionRecruitmentCustomId('lane', recruitmentId),
			values: ['JUNGLE'],
			inCachedGuild: () => true,
			isChatInputCommand: () => false,
			isButton: () => false,
			isStringSelectMenu: () => true,
			isRepliable: () => true,
			deferUpdate,
			editReply,
			reply: vi.fn(async () => undefined),
			followUp: vi.fn(async () => undefined)
		} as unknown as Interaction,
		deferUpdate,
		editReply
	};
}

describe('auction recruitment interaction dispatch', () => {
	it('joins from the auction card using the actor snapshot and interaction idempotency key', async () => {
		const joinAuctionRecruitment = vi.fn(async () => snapshot);
		const leaveAuctionRecruitment = vi.fn(async () => snapshot);
		const current = auctionButton('join');

		await dispatchInteraction(
			current.value,
			context(joinAuctionRecruitment, leaveAuctionRecruitment)
		);

		expect(current.deferUpdate).toHaveBeenCalledOnce();
		expect(joinAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			recruitmentId,
			{
				actor: {
					id: userId,
					globalDisplayName: 'Longcat',
					displayName: 'MSM Longcat',
					avatarHash: null,
					roleIds: ['300000000000000001']
				}
			},
			{ idempotencyKey: interactionId }
		);
		const reply = current.editReply.mock.calls[0]?.[0];
		expect(reply).toEqual(
			expect.objectContaining({
				flags: MessageFlags.IsComponentsV2,
				components: expect.any(Array)
			})
		);
		expect(reply).not.toHaveProperty('embeds');
		expect(reply).not.toHaveProperty('content');
		expect(leaveAuctionRecruitment).not.toHaveBeenCalled();
	});

	it('uses a selected lane as an explicit auction join request', async () => {
		const joinAuctionRecruitment = vi.fn(async () => snapshot);
		const leaveAuctionRecruitment = vi.fn(async () => snapshot);
		const current = auctionLaneSelect();

		await dispatchInteraction(
			current.value,
			context(joinAuctionRecruitment, leaveAuctionRecruitment)
		);

		expect(current.deferUpdate).toHaveBeenCalledOnce();
		expect(joinAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			recruitmentId,
			expect.objectContaining({ declaredLane: 'JUNGLE' }),
			{ idempotencyKey: interactionId }
		);
		const reply = current.editReply.mock.calls[0]?.[0];
		expect(reply).toEqual(
			expect.objectContaining({
				flags: MessageFlags.IsComponentsV2,
				components: expect.any(Array)
			})
		);
	});

	it('leaves through the auction member route with the current actor as the target user', async () => {
		const joinAuctionRecruitment = vi.fn(async () => snapshot);
		const leaveAuctionRecruitment = vi.fn(async () => snapshot);
		const current = auctionButton('leave');

		await dispatchInteraction(
			current.value,
			context(joinAuctionRecruitment, leaveAuctionRecruitment)
		);

		expect(leaveAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			recruitmentId,
			userId,
			expect.objectContaining({ actor: expect.objectContaining({ id: userId }) }),
			{ idempotencyKey: interactionId }
		);
		expect(joinAuctionRecruitment).not.toHaveBeenCalled();
	});
});
