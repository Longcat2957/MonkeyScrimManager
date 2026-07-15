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
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import { auctionRecruitmentForceCancelCommand } from './auction-recruitment-force-cancel.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const tournamentId = '01900000-0000-7000-8000-000000000103';

function context(forceCancelAuctionRecruitment: ReturnType<typeof vi.fn>): InteractionContext {
	return {
		api: { forceCancelAuctionRecruitment } as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: {} as BotConfig,
		logger: {} as Logger
	};
}

function interaction(value = recruitmentId) {
	const deferReply = vi.fn(async () => undefined);
	const editReply = vi.fn(async () => undefined);
	const reply = vi.fn(async () => undefined);
	return {
		value: {
			id: interactionId,
			guildId,
			guild: {},
			member: { nick: 'MSM Longcat', roles: ['300000000000000001'] },
			user: {
				id: userId,
				username: 'longcat_login',
				globalName: 'Longcat',
				avatar: null
			},
			options: { getString: () => value },
			inCachedGuild: () => true,
			deferReply,
			editReply,
			reply
		} as unknown as ChatInputCommandInteraction,
		deferReply,
		editReply,
		reply
	};
}

describe('/경매강제취소', () => {
	it('defines one required recruitment UUID in guild context', () => {
		const definition = auctionRecruitmentForceCancelCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '경매강제취소',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: '모집',
				type: ApplicationCommandOptionType.String,
				required: true,
				min_length: 36,
				max_length: 36
			})
		]);
	});

	it('force-cancels the linked tournament with the command actor and reports MMR rollback', async () => {
		const forceCancelAuctionRecruitment = vi.fn(async () => ({
			recruitmentId,
			recruitmentStatus: 'CANCELLED' as const,
			recruitmentRevision: 8,
			tournamentId,
			tournamentStatus: 'CANCELLED' as const,
			tournamentRevision: 11,
			reversedGameCount: 2
		}));
		const current = interaction();

		await auctionRecruitmentForceCancelCommand.execute(
			current.value,
			context(forceCancelAuctionRecruitment)
		);

		expect(current.deferReply).toHaveBeenCalledWith({ flags: MessageFlags.Ephemeral });
		expect(forceCancelAuctionRecruitment).toHaveBeenCalledWith(
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
		expect(current.editReply).toHaveBeenCalledWith(
			'경매 모집과 연결된 경매 내전을 강제 취소했습니다. 확정된 세트 2개와 MMR 반영을 되돌렸습니다.'
		);
	});

	it('rejects a malformed recruitment ID before making an API request', async () => {
		const forceCancelAuctionRecruitment = vi.fn();
		const current = interaction('214');

		await auctionRecruitmentForceCancelCommand.execute(
			current.value,
			context(forceCancelAuctionRecruitment)
		);

		expect(current.deferReply).not.toHaveBeenCalled();
		expect(forceCancelAuctionRecruitment).not.toHaveBeenCalled();
		expect(current.reply).toHaveBeenCalledWith({
			content: '모집 ID는 경매 모집 페이지 URL에 있는 UUID 형식이어야 합니다.',
			flags: MessageFlags.Ephemeral
		});
	});
});
