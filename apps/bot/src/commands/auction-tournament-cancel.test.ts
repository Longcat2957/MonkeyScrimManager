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
import { auctionTournamentCancelCommand } from './auction-tournament-cancel.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const tournamentId = '01900000-0000-7000-8000-000000000103';

const preview = {
	tournament: {
		id: tournamentId,
		displayNumber: 216,
		title: '20인 경매 내전 #216',
		status: 'IN_GAME' as const,
		revision: 9
	}
};

const result = {
	tournamentId,
	tournamentStatus: 'CANCELLED' as const,
	tournamentRevision: 10,
	sourceRecruitmentId: '01900000-0000-7000-8000-000000000102',
	sourceRecruitmentRevision: 7,
	reversedGameCount: 2
};

function context(
	getAuctionTournamentCancelPreview: ReturnType<typeof vi.fn>,
	cancelAuctionTournament: ReturnType<typeof vi.fn>
): InteractionContext {
	return {
		api: {
			getAuctionTournamentCancelPreview,
			cancelAuctionTournament
		} as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: {} as BotConfig,
		logger: {} as Logger
	};
}

function interaction(options: { tournamentId?: string; reason?: string } = {}) {
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
			options: {
				getString: (name: string) => {
					if (name === '토너먼트') return options.tournamentId ?? tournamentId;
					if (name === '사유') return options.reason ?? '운영 일정 변경';
					return null;
				}
			},
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

describe('/경매조기종료', () => {
	it('defines required UUID and reason options in guild context', () => {
		const definition = auctionTournamentCancelCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '경매조기종료',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: '토너먼트',
				type: ApplicationCommandOptionType.String,
				required: true,
				min_length: 36,
				max_length: 36
			}),
			expect.objectContaining({
				name: '사유',
				type: ApplicationCommandOptionType.String,
				required: true,
				min_length: 1,
				max_length: 240
			})
		]);
	});

	it('reads the canonical revision before cancelling with the Discord actor and idempotency key', async () => {
		const getAuctionTournamentCancelPreview = vi.fn(async () => preview);
		const cancelAuctionTournament = vi.fn(async () => result);
		const current = interaction({ reason: '  운영 일정 변경  ' });

		await auctionTournamentCancelCommand.execute(
			current.value,
			context(getAuctionTournamentCancelPreview, cancelAuctionTournament)
		);

		expect(current.deferReply).toHaveBeenCalledWith({ flags: MessageFlags.Ephemeral });
		expect(getAuctionTournamentCancelPreview).toHaveBeenCalledWith(guildId, tournamentId);
		expect(cancelAuctionTournament).toHaveBeenCalledWith(
			guildId,
			tournamentId,
			{
				actor: {
					id: userId,
					globalDisplayName: 'Longcat',
					displayName: 'MSM Longcat',
					avatarHash: null,
					roleIds: ['300000000000000001']
				},
				expectedRevision: 9,
				reason: '운영 일정 변경'
			},
			{ idempotencyKey: interactionId }
		);
		expect(getAuctionTournamentCancelPreview.mock.invocationCallOrder[0]).toBeLessThan(
			cancelAuctionTournament.mock.invocationCallOrder[0]!
		);
		expect(current.editReply).toHaveBeenCalledWith(
			'경매 내전 #216을 조기 종료했습니다. 확정된 세트 2개와 MMR 반영을 되돌렸습니다.'
		);
	});

	it('rejects a non-UUID tournament value before making an API request', async () => {
		const getAuctionTournamentCancelPreview = vi.fn(async () => preview);
		const cancelAuctionTournament = vi.fn(async () => result);
		const current = interaction({ tournamentId: '216' });

		await auctionTournamentCancelCommand.execute(
			current.value,
			context(getAuctionTournamentCancelPreview, cancelAuctionTournament)
		);

		expect(current.deferReply).not.toHaveBeenCalled();
		expect(getAuctionTournamentCancelPreview).not.toHaveBeenCalled();
		expect(cancelAuctionTournament).not.toHaveBeenCalled();
		expect(current.reply).toHaveBeenCalledWith({
			content: '토너먼트 ID는 경매 내전 페이지 URL에 있는 UUID 형식이어야 합니다.',
			flags: MessageFlags.Ephemeral
		});
	});
});
