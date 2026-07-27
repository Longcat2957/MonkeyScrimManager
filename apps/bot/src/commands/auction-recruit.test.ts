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
import { auctionRecruitCommand } from './auction-recruit.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const expectedStartInput = '2026-07-26 20:00';
const expectedStartAt = '2026-07-26T20:00:00+09:00';

const snapshot: AuctionRecruitmentSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: `20인 경매 내전 모집 #214 · 예상 시작 ${expectedStartAt} (KST)`,
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
	participantCount: number,
	options: {
		semifinalBestOf?: number;
		finalBestOf?: number;
		hardFearless?: boolean;
		expectedStartInput?: string;
	} = {}
) {
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
				getInteger: (name: string) => {
					if (name === '정원') return participantCount;
					if (name === '4강') return options.semifinalBestOf ?? null;
					if (name === '결승') return options.finalBestOf ?? null;
					return null;
				},
				getBoolean: (name: string) =>
					name === '하드피어리스' ? (options.hardFearless ?? null) : null,
				getString: (name: string) =>
					name === '예상시작' ? (options.expectedStartInput ?? expectedStartInput) : null
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

describe('/경매모집', () => {
	it('requires a 10-person or 20-person capacity in guild context', () => {
		const definition = auctionRecruitCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '경매모집',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: '정원',
				type: ApplicationCommandOptionType.Integer,
				required: true,
				choices: [
					{ name: '10인', value: 10 },
					{ name: '20인', value: 20 }
				]
			}),
			expect.objectContaining({
				name: '예상시작',
				type: ApplicationCommandOptionType.String,
				required: true
			}),
			expect.objectContaining({
				name: '결승',
				type: ApplicationCommandOptionType.Integer,
				choices: [
					{ name: 'BO1', value: 1 },
					{ name: 'BO3', value: 3 },
					{ name: 'BO5', value: 5 }
				]
			}),
			expect.objectContaining({
				name: '4강',
				type: ApplicationCommandOptionType.Integer,
				choices: [
					{ name: 'BO1', value: 1 },
					{ name: 'BO3', value: 3 },
					{ name: 'BO5', value: 5 }
				]
			}),
			expect.objectContaining({ name: '하드피어리스', type: ApplicationCommandOptionType.Boolean })
		]);
	});

	it('creates the selected auction format and rules, then returns an ephemeral web link', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction(20, { semifinalBestOf: 1, finalBestOf: 5, hardFearless: true });

		await auctionRecruitCommand.execute(current.value, context(createAuctionRecruitment));

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

	it('uses BO3 and hard fearless by default for a 10-person auction', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction(10);

		await auctionRecruitCommand.execute(current.value, context(createAuctionRecruitment));

		expect(createAuctionRecruitment).toHaveBeenCalledWith(
			guildId,
			expect.objectContaining({
				format: 'TEN_PERSON',
				semifinalBestOf: null,
				finalBestOf: 3,
				hardFearless: true,
				expectedStartAt
			}),
			{ idempotencyKey: interactionId }
		);
	});

	it('rejects a semifinal format for a 10-person auction before creating it', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction(10, { semifinalBestOf: 3 });

		await auctionRecruitCommand.execute(current.value, context(createAuctionRecruitment));

		expect(createAuctionRecruitment).not.toHaveBeenCalled();
		expect(current.deferReply).not.toHaveBeenCalled();
		expect(serializedReply(current.reply).flags).toBe(
			MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		);
		expect(renderedReply(current.reply)).toContain(
			'10인 경매 내전은 4강이 없으므로 `4강` 형식을 설정할 수 없습니다.'
		);
	});

	it('rejects an invalid KST expected-start value before creating an auction', async () => {
		const createAuctionRecruitment = vi.fn(async () => snapshot);
		const current = interaction(20, { expectedStartInput: '2026-02-30 20:00' });

		await auctionRecruitCommand.execute(current.value, context(createAuctionRecruitment));

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
