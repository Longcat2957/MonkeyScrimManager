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
import { auctionRecruitTenCommand, auctionRecruitTwentyCommand } from './auction-recruit.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const expectedStartInput = '2026-07-26 20:00';
const expectedStartAt = '2026-07-26T20:00:00+09:00';
const defaultExpectedStartAt = '2026-07-26T19:30:00+09:00';

const snapshot: AuctionRecruitmentSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '20인 경매 내전 모집 #214 · 예상 시작 2026-07-26 20:00 (KST)',
		format: 'TWENTY_PERSON',
		rules: { hardFearless: true, semifinalBestOf: 1, finalBestOf: 5 },
		participantCount: 20,
		joinedParticipantCount: 0,
		teamCount: 4,
		status: 'OPEN',
		revision: 0,
		channelId: null,
		messageId: null,
		createdById: userId,
		updatedAt: '2026-07-14T10:00:00Z',
		href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
	},
	participants: [],
	links: { draft: null },
	permissions: { canManageMembers: false, canClose: false, canCancel: true }
};

function context(createAuctionRecruitment: ReturnType<typeof vi.fn>): InteractionContext {
	return {
		api: { createAuctionRecruitment } as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
		logger: {} as Logger
	};
}

function interaction(
	options: {
		bestOf?: number;
		semifinalBestOf?: number;
		finalBestOf?: number;
		hardFearless?: boolean;
		expectedStartInput?: string | null;
	} = {}
) {
	const deferReply = vi.fn(async () => undefined);
	const editReply = vi.fn(async () => undefined);
	const reply = vi.fn(async () => undefined);
	return {
		value: {
			id: interactionId,
			createdAt: new Date('2026-07-26T10:00:00.000Z'),
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
				getInteger: (name: string) => {
					if (name === '형식') return options.bestOf ?? null;
					if (name === '4강') return options.semifinalBestOf ?? null;
					if (name === '결승') return options.finalBestOf ?? null;
					return null;
				},
				getBoolean: (name: string) =>
					name === '하드피어리스' ? (options.hardFearless ?? null) : null,
				getString: (name: string) =>
					name === '예상시작'
						? options.expectedStartInput === undefined
							? expectedStartInput
							: options.expectedStartInput
						: null
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

describe('/경매내전_10인, /경매내전_20인', () => {
	it('registers dedicated 10-person and 20-person commands', () => {
		const tenPersonDefinition = auctionRecruitTenCommand.data.toJSON();
		const twentyPersonDefinition = auctionRecruitTwentyCommand.data.toJSON();

		expect(tenPersonDefinition).toMatchObject({
			name: '경매내전_10인',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(tenPersonDefinition.options).toEqual([
			expect.objectContaining({
				name: '예상시작',
				type: ApplicationCommandOptionType.String,
				required: false
			}),
			expect.objectContaining({
				name: '형식',
				type: ApplicationCommandOptionType.Integer
			}),
			expect.objectContaining({
				name: '하드피어리스',
				type: ApplicationCommandOptionType.Boolean
			})
		]);
		expect(tenPersonDefinition.options).not.toContainEqual(
			expect.objectContaining({ name: '4강' })
		);
		expect(tenPersonDefinition.options).not.toContainEqual(
			expect.objectContaining({ name: '결승' })
		);

		expect(twentyPersonDefinition).toMatchObject({
			name: '경매내전_20인',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(twentyPersonDefinition.options).toEqual([
			expect.objectContaining({
				name: '예상시작',
				type: ApplicationCommandOptionType.String,
				required: false
			}),
			expect.objectContaining({ name: '4강', type: ApplicationCommandOptionType.Integer }),
			expect.objectContaining({ name: '결승', type: ApplicationCommandOptionType.Integer }),
			expect.objectContaining({
				name: '하드피어리스',
				type: ApplicationCommandOptionType.Boolean
			})
		]);
	});

	it('creates a 20-person auction with semifinal and final rules', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction({
			semifinalBestOf: 1,
			finalBestOf: 5,
			hardFearless: true
		});

		await auctionRecruitTwentyCommand.execute(current.value, context(createAuctionRecruitment));

		expect(current.deferReply).toHaveBeenCalledWith({
			flags: MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		});
		expect(createAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			{
				actor: {
					id: userId,
					globalDisplayName: 'Longcat',
					displayName: 'MSM Longcat',
					avatarHash: null,
					roleIds: ['300000000000000001']
				},
				format: 'TWENTY_PERSON',
				semifinalBestOf: 1,
				finalBestOf: 5,
				hardFearless: true,
				expectedStartAt
			},
			{ idempotencyKey: interactionId }
		);
		expect(serializedReply(current.editReply).flags).toBe(MessageFlags.IsComponentsV2);
		expect(renderedReply(current.editReply)).toContain('20인 경매 모집 생성');
		expect(renderedReply(current.editReply)).toContain('결승');
		expect(renderedReply(current.editReply)).toContain('BO5');
		expect(renderedReply(current.editReply)).toContain('4강');
		expect(renderedReply(current.editReply)).toContain('BO1');
		expect(renderedReply(current.editReply)).toContain('하드 피어리스');
		expect(renderedReply(current.editReply)).toContain(expectedStartAt);
		expect(renderedReply(current.editReply)).toContain(
			`http://localhost:5173/g/${guildId}/auction/recruitments/${recruitmentId}`
		);
	});

	it('uses the generic series format for a 10-person auction', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction({ bestOf: 5 });

		await auctionRecruitTenCommand.execute(current.value, context(createAuctionRecruitment));

		expect(createAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			expect.objectContaining({
				format: 'TEN_PERSON',
				semifinalBestOf: null,
				finalBestOf: 5,
				hardFearless: true,
				expectedStartAt
			}),
			{ idempotencyKey: interactionId }
		);
		expect(renderedReply(current.editReply)).toContain('형식');
		expect(renderedReply(current.editReply)).not.toContain('결승');
		expect(renderedReply(current.editReply)).not.toContain('4강');
	});

	it('uses the interaction time plus 30 minutes when expected start is omitted', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction({ expectedStartInput: null });

		await auctionRecruitTwentyCommand.execute(current.value, context(createAuctionRecruitment));

		expect(createAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			expect.objectContaining({ expectedStartAt: defaultExpectedStartAt }),
			{ idempotencyKey: interactionId }
		);
	});

	it('rejects an invalid KST expected-start value before creating an auction', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction({ expectedStartInput: '2026-02-30 20:00' });

		await auctionRecruitTwentyCommand.execute(current.value, context(createAuctionRecruitment));

		expect(createAuctionRecruitment).not.toHaveBeenCalled();
		expect(current.deferReply).not.toHaveBeenCalled();
		expect(renderedReply(current.reply)).toContain('KST 24시간 형식');
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
