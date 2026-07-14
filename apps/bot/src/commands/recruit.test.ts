import {
	ApplicationCommandOptionType,
	ApplicationIntegrationType,
	InteractionContextType,
	type ChatInputCommandInteraction
} from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import type { MonkeyBotApiClient } from '../api/client.js';
import type { RecruitmentSnapshot } from '../api/contracts.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import { recruitCommand } from './recruit.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';

const snapshot: RecruitmentSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '금요 정기 내전',
		status: 'OPEN',
		targetCount: 10,
		bestOf: 3,
		hardFearless: false,
		revision: 0,
		channelId: null,
		messageId: null,
		createdById: userId,
		updatedAt: '2026-07-14T10:00:00Z'
	},
	participants: [],
	links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
};

function context(
	createRecruitment: ReturnType<typeof vi.fn>,
	bindRecruitmentMessage: ReturnType<typeof vi.fn>
): InteractionContext {
	return {
		api: { createRecruitment, bindRecruitmentMessage } as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
		logger: {} as Logger
	};
}

function interaction(bestOf: number | null, hardFearless: boolean | null) {
	const editReply = vi.fn(async () => ({
		channelId: '500000000000000001',
		id: '600000000000000001'
	}));
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
				getInteger: () => bestOf,
				getBoolean: () => hardFearless
			},
			inCachedGuild: () => true,
			deferReply: vi.fn(async () => undefined),
			editReply,
			reply: vi.fn(async () => undefined)
		} as unknown as ChatInputCommandInteraction,
		editReply
	};
}

describe('/내전모집', () => {
	it('defines optional series-format and hard-fearless options', () => {
		const definition = recruitCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '내전모집',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: '형식',
				type: ApplicationCommandOptionType.Integer,
				required: false,
				choices: [
					{ name: 'BO1', value: 1 },
					{ name: 'BO3', value: 3 },
					{ name: 'BO5', value: 5 }
				]
			}),
			expect.objectContaining({
				name: '하드피어리스',
				type: ApplicationCommandOptionType.Boolean,
				required: false
			})
		]);
	});

	it.each([
		{ bestOf: null, hardFearless: null, expectedBestOf: 3, expectedHardFearless: false },
		{ bestOf: 5, hardFearless: true, expectedBestOf: 5, expectedHardFearless: true }
	])('sends selected rules to the recruitment API', async (testCase) => {
		const createRecruitment = vi.fn(async () => snapshot);
		const bindRecruitmentMessage = vi.fn(async () => snapshot);
		const current = interaction(testCase.bestOf, testCase.hardFearless);

		await recruitCommand.execute(current.value, context(createRecruitment, bindRecruitmentMessage));

		expect(createRecruitment).toHaveBeenCalledWith(
			guildId,
			{
				actor: {
					id: userId,
					globalDisplayName: 'Longcat',
					displayName: 'MSM Longcat',
					avatarHash: null,
					roleIds: ['300000000000000001']
				},
				bestOf: testCase.expectedBestOf,
				hardFearless: testCase.expectedHardFearless
			},
			{ idempotencyKey: interactionId }
		);
		expect(bindRecruitmentMessage).toHaveBeenCalledWith(
			guildId,
			recruitmentId,
			expect.objectContaining({
				channelId: '500000000000000001',
				messageId: '600000000000000001'
			}),
			{ idempotencyKey: interactionId }
		);
	});
});
