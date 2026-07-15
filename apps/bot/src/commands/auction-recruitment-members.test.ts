import {
	ApplicationCommandOptionType,
	ApplicationIntegrationType,
	InteractionContextType,
	MessageFlags,
	type ChatInputCommandInteraction
} from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import type { MonkeyBotApiClient } from '../api/client.js';
import type { AuctionRecruitmentSnapshot } from '../api/contracts.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import {
	auctionRecruitmentAddMemberCommand,
	auctionRecruitmentRemoveMemberCommand
} from './auction-recruitment-members.js';

const guildId = '200000000000000001';
const operatorId = '100000000000000001';
const targetId = '100000000000000002';
const interactionId = '400000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';

const snapshot: AuctionRecruitmentSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '10인 경매 내전 모집 #214',
		format: 'TEN_PERSON',
		rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
		participantCount: 10,
		joinedParticipantCount: 1,
		teamCount: 2,
		status: 'OPEN',
		revision: 1,
		channelId: null,
		messageId: null,
		createdById: operatorId,
		updatedAt: '2026-07-15T10:00:00Z',
		href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
	},
	participants: [
		{
			id: targetId,
			displayName: 'Target Player',
			declaredLane: 'JUNGLE'
		}
	],
	links: { draft: null },
	permissions: { canManageMembers: true, canClose: true, canCancel: true }
};

function context(
	forceAddAuctionRecruitmentMember: ReturnType<typeof vi.fn>,
	leaveAuctionRecruitment: ReturnType<typeof vi.fn>
): InteractionContext {
	return {
		api: {
			forceAddAuctionRecruitmentMember,
			leaveAuctionRecruitment
		} as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: {} as BotConfig,
		logger: {} as Logger
	};
}

function interaction(options: { recruitmentId?: string; lane?: string | null } = {}) {
	const deferReply = vi.fn(async () => undefined);
	const editReply = vi.fn(async () => undefined);
	const reply = vi.fn(async () => undefined);
	const targetUser = {
		id: targetId,
		username: 'target_login',
		globalName: 'Target Player',
		avatar: 'target-avatar'
	};
	const targetMember = {
		displayName: 'Target Player (MSM)',
		roles: { cache: new Map([['300000000000000002', {}]]) }
	};
	const fetch = vi.fn(async () => targetMember);
	return {
		value: {
			id: interactionId,
			guildId,
			guild: { members: { fetch } },
			member: { nick: 'MSM Longcat', roles: ['300000000000000001'] },
			user: {
				id: operatorId,
				username: 'longcat_login',
				globalName: 'Longcat',
				avatar: null
			},
			options: {
				getString: (name: string) => {
					if (name === '모집') return options.recruitmentId ?? recruitmentId;
					if (name === '라인') return options.lane ?? 'JUNGLE';
					return null;
				},
				getUser: () => targetUser
			},
			inCachedGuild: () => true,
			deferReply,
			editReply,
			reply
		} as unknown as ChatInputCommandInteraction,
		deferReply,
		editReply,
		reply,
		fetch
	};
}

describe('/경매인원추가', () => {
	it('defines the required recruitment and member options before the optional lane', () => {
		const definition = auctionRecruitmentAddMemberCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '경매인원추가',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: '모집',
				type: ApplicationCommandOptionType.String,
				required: true
			}),
			expect.objectContaining({
				name: '멤버',
				type: ApplicationCommandOptionType.User,
				required: true
			}),
			expect.objectContaining({ name: '라인', type: ApplicationCommandOptionType.String })
		]);
	});

	it('forces a current guild member into the auction recruitment with the operator actor', async () => {
		const forceAddAuctionRecruitmentMember = vi.fn(async () => snapshot);
		const leaveAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction();

		await auctionRecruitmentAddMemberCommand.execute(
			current.value,
			context(forceAddAuctionRecruitmentMember, leaveAuctionRecruitment)
		);

		expect(current.fetch).toHaveBeenCalledWith(targetId);
		expect(current.deferReply).toHaveBeenCalledWith({
			flags: MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		});
		expect(forceAddAuctionRecruitmentMember).toHaveBeenCalledWith(
			guildId,
			recruitmentId,
			{
				actor: {
					id: operatorId,
					globalDisplayName: 'Longcat',
					displayName: 'MSM Longcat',
					avatarHash: null,
					roleIds: ['300000000000000001']
				},
				member: {
					id: targetId,
					globalDisplayName: 'Target Player',
					displayName: 'Target Player (MSM)',
					avatarHash: 'target-avatar',
					roleIds: ['300000000000000002']
				},
				declaredLane: 'JUNGLE'
			},
			{ idempotencyKey: interactionId }
		);
		expect(serializedReply(current.editReply).flags).toBe(MessageFlags.IsComponentsV2);
		expect(renderedReply(current.editReply)).toContain('경매 모집 참가자 추가');
		expect(renderedReply(current.editReply)).toContain(
			'Target Player (MSM)님을 경매 모집에 추가했습니다.'
		);
		expect(renderedReply(current.editReply)).toContain('현재 참가 인원');
		expect(renderedReply(current.editReply)).toContain('1명');
	});

	it('rejects a malformed recruitment ID before loading the target member', async () => {
		const forceAddAuctionRecruitmentMember = vi.fn(async () => snapshot);
		const leaveAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction({ recruitmentId: '214' });

		await auctionRecruitmentAddMemberCommand.execute(
			current.value,
			context(forceAddAuctionRecruitmentMember, leaveAuctionRecruitment)
		);

		expect(current.fetch).not.toHaveBeenCalled();
		expect(forceAddAuctionRecruitmentMember).not.toHaveBeenCalled();
		expect(serializedReply(current.reply).flags).toBe(
			MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		);
		expect(renderedReply(current.reply)).toContain(
			'모집 ID는 경매 모집 페이지 URL에 있는 UUID 형식이어야 합니다.'
		);
	});
});

describe('/경매인원삭제', () => {
	it('uses the existing leave route with the selected member ID', async () => {
		const forceAddAuctionRecruitmentMember = vi.fn(async () => snapshot);
		const leaveAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction();

		await auctionRecruitmentRemoveMemberCommand.execute(
			current.value,
			context(forceAddAuctionRecruitmentMember, leaveAuctionRecruitment)
		);

		expect(leaveAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			recruitmentId,
			targetId,
			{
				actor: {
					id: operatorId,
					globalDisplayName: 'Longcat',
					displayName: 'MSM Longcat',
					avatarHash: null,
					roleIds: ['300000000000000001']
				}
			},
			{ idempotencyKey: interactionId }
		);
		expect(serializedReply(current.editReply).flags).toBe(MessageFlags.IsComponentsV2);
		expect(renderedReply(current.editReply)).toContain('경매 모집 참가자 제거');
		expect(renderedReply(current.editReply)).toContain(
			'Target Player님을 경매 모집에서 제거했습니다.'
		);
	});
});

function serializedReply(reply: ReturnType<typeof vi.fn>): {
	flags: number;
	components: unknown[];
} {
	return JSON.parse(JSON.stringify(reply.mock.calls[0]?.[0])) as {
		flags: number;
		components: unknown[];
	};
}

function renderedReply(reply: ReturnType<typeof vi.fn>): string {
	return JSON.stringify(serializedReply(reply));
}
